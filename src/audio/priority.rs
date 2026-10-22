use crate::config::PersistentState;
use crate::notification::NotificationThrottler;
use crate::types::{
    CommunicationRoute, DeviceId, DeviceRole, DeviceType, PriorityKind, TemporaryPriorities,
};

use super::AudioBackend;
use super::presence::HeadsetPresence;

pub fn enforce_priorities(
    backend: &impl AudioBackend,
    state: &PersistentState,
    throttler: &mut NotificationThrottler,
    temporary_priorities: &TemporaryPriorities,
) {
    for device_type in [DeviceType::Output, DeviceType::Input] {
        enforce_priority_for_type(
            backend,
            device_type,
            state,
            temporary_priorities.get(device_type),
            throttler,
        );
    }
}

fn is_default_device(
    backend: &impl AudioBackend,
    device_type: DeviceType,
    role: DeviceRole,
    target_id: &DeviceId,
) -> bool {
    match backend.default_device(device_type, role) {
        Ok(d) => *target_id == *d.id(),
        Err(e) => {
            log::warn!("Failed to get default {device_type} {role} device: {e:#}");
            false
        }
    }
}

fn enforce_priority_for_type(
    backend: &impl AudioBackend,
    device_type: DeviceType,
    state: &PersistentState,
    temporary_priority: Option<&DeviceId>,
    throttler: &mut NotificationThrottler,
) {
    let mut priority_list = state.priority_list(device_type).to_vec();
    if let Some(temp_id) = temporary_priority {
        priority_list.insert(0, temp_id.clone());
    }

    let main_target = find_highest_priority_active_device(backend, &priority_list, state);
    // With no priority order, explicit pairs can follow a manual Windows output
    // switch without claiming or changing the multimedia/console defaults.
    let routing_target = main_target.clone().or_else(|| {
        if device_type != DeviceType::Output || !priority_list.is_empty() {
            return None;
        }
        let current = backend
            .default_device(device_type, DeviceRole::Console)
            .ok()?;
        let settings = state.device_settings(current.id())?;
        (!settings.communication_route.is_inherited()
            && device_is_available(backend, current.id(), state))
        .then(|| current.id().clone())
    });
    let comm_target =
        resolve_communication_target(backend, device_type, state, routing_target.as_ref());

    // Devices that were actually switched to, paired with the list that chose them.
    let mut switched: Vec<(DeviceId, PriorityKind)> = Vec::new();

    // Enforce Console and Multimedia roles together
    if let Some(target_id) = &main_target
        && !is_default_device(backend, device_type, DeviceRole::Console, target_id)
    {
        log::info!("Enforcing {device_type} priority: Switching to {target_id}");
        for role in [DeviceRole::Console, DeviceRole::Multimedia] {
            if let Err(e) = backend.set_default_device(target_id, role) {
                log::error!(
                    "Failed to set default {role} {device_type} device to {target_id}: {e:#}"
                );
            }
        }
        switched.push((target_id.clone(), PriorityKind::Main));
    }

    if let Some(target_id) = &comm_target
        && !is_default_device(backend, device_type, DeviceRole::Communications, target_id)
    {
        log::info!("Enforcing {device_type} priority (Communication): Switching to {target_id}");
        if let Err(e) = backend.set_default_device(target_id, DeviceRole::Communications) {
            log::error!(
                "Failed to set default {device_type} communications device to {target_id}: {e:#}"
            );
        }
        // Only report the communication switch separately when it landed on a
        // different device than the main roles did.
        if !switched.iter().any(|(id, _)| id == target_id) {
            switched.push((target_id.clone(), PriorityKind::Communication));
        }
    }

    if state.notify_on_priority_restore(device_type) {
        for (target_id, kind) in switched {
            notify_priority_restored(backend, device_type, kind, &target_id, throttler);
        }
    }
}

/// Determines which device the Communications role should resolve to.
///
/// A non-empty communication priority list takes over the role entirely. When
/// the list is empty the role mirrors the main list, gated on the
/// "Also switch default communication device" preference — the pre-existing
/// behavior.
fn resolve_communication_target(
    backend: &impl AudioBackend,
    device_type: DeviceType,
    state: &PersistentState,
    main_target: Option<&DeviceId>,
) -> Option<DeviceId> {
    if device_type == DeviceType::Output
        && let Some(main_id) = main_target
        && let Some(settings) = state.device_settings(main_id)
    {
        match &settings.communication_route {
            CommunicationRoute::SameDevice => return Some(main_id.clone()),
            CommunicationRoute::Paired(paired_id) => {
                // A missing voice endpoint falls back to the chosen main output,
                // rather than leaving chat on an unrelated previous device.
                let paired_is_available = backend
                    .devices(DeviceType::Output)
                    .is_ok_and(|devices| devices.iter().any(|d| d.id() == paired_id))
                    && device_is_available(backend, paired_id, state);
                return Some(if paired_is_available {
                    paired_id.clone()
                } else {
                    main_id.clone()
                });
            }
            CommunicationRoute::Inherited => {}
        }
    }
    let comm_list = state.communication_priority_list(device_type);
    if comm_list.is_empty() {
        return if state.switch_communication_device(device_type) {
            main_target.cloned()
        } else {
            None
        };
    }
    find_highest_priority_active_device(backend, comm_list, state)
}

fn notify_priority_restored(
    backend: &impl AudioBackend,
    device_type: DeviceType,
    kind: PriorityKind,
    target_id: &DeviceId,
    throttler: &mut NotificationThrottler,
) {
    let device_name = backend.device_by_id(target_id).map_or_else(
        |e| {
            log::warn!("Could not get name for device {target_id}: {e:#}");
            "Unknown Device".to_string()
        },
        |d| d.name(),
    );
    let title = match (device_type, kind) {
        (DeviceType::Output, PriorityKind::Main) => "Default Output Device Restored",
        (DeviceType::Input, PriorityKind::Main) => "Default Input Device Restored",
        (DeviceType::Output, PriorityKind::Communication) => {
            "Default Output Communication Device Restored"
        }
        (DeviceType::Input, PriorityKind::Communication) => {
            "Default Input Communication Device Restored"
        }
    };
    let throttle_key = match kind {
        PriorityKind::Main => format!("priority_restore_{target_id}"),
        PriorityKind::Communication => format!("priority_restore_communication_{target_id}"),
    };
    throttler.send_if_not_throttled(
        &throttle_key,
        title,
        &format!("Switched to {device_name} based on priority list."),
    );
}

fn find_highest_priority_active_device(
    backend: &impl AudioBackend,
    priority_list: &[DeviceId],
    state: &PersistentState,
) -> Option<DeviceId> {
    priority_list
        .iter()
        .find(|device_id| device_is_available(backend, device_id, state))
        .cloned()
}

fn device_is_available(
    backend: &impl AudioBackend,
    device_id: &DeviceId,
    state: &PersistentState,
) -> bool {
    if let Some(settings) = state.device_settings(device_id)
        && backend.headset_presence(settings.presence_source) == HeadsetPresence::Unavailable
    {
        return false;
    }
    match backend.device_by_id(device_id) {
        Ok(device) => match device.is_active() {
            Ok(active) => active,
            Err(e) => {
                log::warn!("Failed to check if device {device_id} is active: {e:#}");
                false
            }
        },
        Err(e) => {
            log::warn!("Failed to get device {device_id} for priority check: {e:#}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::tests::{MockAudioBackend, MockDevice};
    use crate::notification::NotificationThrottler;

    #[test]
    fn enforce_priorities_switches_to_highest_active() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("dev_a", "Device A", true),
            MockDevice::new("dev_b", "Device B", true),
        ]);
        backend.set_default("dev_b", DeviceType::Output);

        let mut state = PersistentState::default();
        state.output.priority_list = vec!["dev_a".into(), "dev_b".into()];

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: None,
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        assert_eq!(
            backend.default_console.borrow().get(&DeviceType::Output),
            Some(&"dev_a".to_string())
        );
    }

    #[test]
    fn enforce_priorities_no_switch_when_correct() {
        let backend = MockAudioBackend::new(vec![MockDevice::new("dev_a", "Device A", true)]);
        backend.set_default("dev_a", DeviceType::Output);

        let mut state = PersistentState::default();
        state.output.priority_list = vec!["dev_a".into()];

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: None,
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        assert_eq!(
            backend.default_console.borrow().get(&DeviceType::Output),
            Some(&"dev_a".to_string())
        );
    }

    #[test]
    fn enforce_priorities_skips_inactive_devices() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("dev_a", "Device A", false),
            MockDevice::new("dev_b", "Device B", true),
        ]);
        backend.set_default("dev_b", DeviceType::Output);

        let mut state = PersistentState::default();
        state.output.priority_list = vec!["dev_a".into(), "dev_b".into()];

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: None,
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        assert_eq!(
            backend.default_console.borrow().get(&DeviceType::Output),
            Some(&"dev_b".to_string())
        );
    }

    #[test]
    fn enforce_priorities_temporary_priority_overrides() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("dev_a", "Device A", true),
            MockDevice::new("dev_temp", "Temp Device", true),
        ]);
        backend.set_default("dev_a", DeviceType::Output);

        let mut state = PersistentState::default();
        state.output.priority_list = vec!["dev_a".into()];

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: Some("dev_temp".into()),
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        assert_eq!(
            backend.default_console.borrow().get(&DeviceType::Output),
            Some(&"dev_temp".to_string())
        );
    }

    #[test]
    fn enforce_priorities_empty_list_does_nothing() {
        let backend = MockAudioBackend::new(vec![MockDevice::new("dev_a", "Device A", true)]);
        backend.set_default("dev_a", DeviceType::Output);

        let state = PersistentState::default();

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: None,
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        assert_eq!(
            backend.default_console.borrow().get(&DeviceType::Output),
            Some(&"dev_a".to_string())
        );
    }

    #[test]
    fn enforce_priorities_communication_device_switching() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("dev_a", "Device A", true),
            MockDevice::new("dev_b", "Device B", true),
        ]);
        backend.set_default("dev_b", DeviceType::Output);

        let mut state = PersistentState::default();
        state.output.priority_list = vec!["dev_a".into(), "dev_b".into()];
        state.output.switch_communication_device = true;

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: None,
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        assert_eq!(
            backend
                .default_communications
                .borrow()
                .get(&DeviceType::Output),
            Some(&"dev_a".to_string())
        );
    }

    #[test]
    fn communication_list_routes_roles_independently() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("speakers", "Speakers", true),
            MockDevice::new("headset", "Headset", true),
        ]);
        backend.set_default("headset", DeviceType::Output);

        let mut state = PersistentState::default();
        state.output.priority_list = vec!["speakers".into()];
        state.output.communication_priority_list = vec!["headset".into()];

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: None,
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        assert_eq!(
            backend.default_console.borrow().get(&DeviceType::Output),
            Some(&"speakers".to_string())
        );
        assert_eq!(
            backend
                .default_communications
                .borrow()
                .get(&DeviceType::Output),
            Some(&"headset".to_string())
        );
    }

    #[test]
    fn communication_list_overrides_switch_communication_preference() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("speakers", "Speakers", true),
            MockDevice::new("headset", "Headset", true),
        ]);
        backend.set_default("speakers", DeviceType::Output);

        let mut state = PersistentState::default();
        state.output.priority_list = vec!["speakers".into()];
        state.output.communication_priority_list = vec!["headset".into()];
        // Even with mirroring switched off, a configured list still drives the role.
        state.output.switch_communication_device = false;

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: None,
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        assert_eq!(
            backend
                .default_communications
                .borrow()
                .get(&DeviceType::Output),
            Some(&"headset".to_string())
        );
    }

    #[test]
    fn communication_list_skips_inactive_devices() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("speakers", "Speakers", true),
            MockDevice::new("headset", "Headset", false),
            MockDevice::new("earbuds", "Earbuds", true),
        ]);
        backend.set_default("speakers", DeviceType::Output);

        let mut state = PersistentState::default();
        state.output.priority_list = vec!["speakers".into()];
        state.output.communication_priority_list = vec!["headset".into(), "earbuds".into()];

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: None,
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        assert_eq!(
            backend
                .default_communications
                .borrow()
                .get(&DeviceType::Output),
            Some(&"earbuds".to_string())
        );
    }

    #[test]
    fn communication_list_applies_without_a_main_list() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("speakers", "Speakers", true),
            MockDevice::new("headset", "Headset", true),
        ]);
        backend.set_default("speakers", DeviceType::Output);

        let mut state = PersistentState::default();
        state.output.communication_priority_list = vec!["headset".into()];

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: None,
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        // Console/Multimedia are left alone, only Communications moves.
        assert_eq!(
            backend.default_console.borrow().get(&DeviceType::Output),
            Some(&"speakers".to_string())
        );
        assert_eq!(
            backend
                .default_communications
                .borrow()
                .get(&DeviceType::Output),
            Some(&"headset".to_string())
        );
    }

    #[test]
    fn empty_communication_list_preserves_mirroring() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("dev_a", "Device A", true),
            MockDevice::new("dev_b", "Device B", true),
        ]);
        backend.set_default("dev_b", DeviceType::Output);

        let mut state = PersistentState::default();
        state.output.priority_list = vec!["dev_a".into()];
        state.output.switch_communication_device = true;

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: None,
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        assert_eq!(
            backend
                .default_communications
                .borrow()
                .get(&DeviceType::Output),
            Some(&"dev_a".to_string())
        );
    }

    #[test]
    fn temporary_priority_does_not_hijack_communication_list() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("speakers", "Speakers", true),
            MockDevice::new("headset", "Headset", true),
            MockDevice::new("dev_temp", "Temp Device", true),
        ]);
        backend.set_default("speakers", DeviceType::Output);

        let mut state = PersistentState::default();
        state.output.priority_list = vec!["speakers".into()];
        state.output.communication_priority_list = vec!["headset".into()];

        let mut times = NotificationThrottler::new();
        let temp = TemporaryPriorities {
            output: Some("dev_temp".into()),
            input: None,
        };

        enforce_priorities(&backend, &state, &mut times, &temp);

        assert_eq!(
            backend.default_console.borrow().get(&DeviceType::Output),
            Some(&"dev_temp".to_string())
        );
        assert_eq!(
            backend
                .default_communications
                .borrow()
                .get(&DeviceType::Output),
            Some(&"headset".to_string())
        );
    }

    #[test]
    fn find_highest_returns_first_active() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("dev_a", "A", false),
            MockDevice::new("dev_b", "B", true),
            MockDevice::new("dev_c", "C", true),
        ]);
        let list = vec!["dev_a".into(), "dev_b".into(), "dev_c".into()];
        assert_eq!(
            find_highest_priority_active_device(&backend, &list, &PersistentState::default()),
            Some(DeviceId::from("dev_b"))
        );
    }

    #[test]
    fn find_highest_returns_none_when_all_inactive() {
        let backend = MockAudioBackend::new(vec![
            MockDevice::new("dev_a", "A", false),
            MockDevice::new("dev_b", "B", false),
        ]);
        let list = vec!["dev_a".into(), "dev_b".into()];
        assert_eq!(
            find_highest_priority_active_device(&backend, &list, &PersistentState::default()),
            None
        );
    }

    #[test]
    fn find_highest_returns_none_for_empty_list() {
        let backend = MockAudioBackend::new(vec![]);
        let list: Vec<DeviceId> = vec![];
        assert_eq!(
            find_highest_priority_active_device(&backend, &list, &PersistentState::default()),
            None
        );
    }
}

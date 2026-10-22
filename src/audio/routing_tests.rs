use super::tests::{MockAudioBackend, MockDevice};
use super::{enforce_priorities, migrate_device_ids, presence::HeadsetPresence};
use crate::config::PersistentState;
use crate::notification::NotificationThrottler;
use crate::types::{CommunicationRoute, DeviceType, PresenceSource, TemporaryPriorities};

fn setup(voice_active: bool) -> (MockAudioBackend, PersistentState) {
    let backend = MockAudioBackend::new(vec![
        MockDevice::new("game", "A50 X Game", true),
        MockDevice::new("voice", "A50 X Voice", voice_active),
        MockDevice::new("speakers", "Speakers", true),
    ]);
    let mut state = PersistentState::default();
    state.output.priority_list = vec!["game".into(), "speakers".into()];
    state
        .ensure_device_settings("game".into(), "A50 X Game".into(), DeviceType::Output)
        .communication_route = CommunicationRoute::Paired("voice".into());
    state.ensure_device_settings("voice".into(), "A50 X Voice".into(), DeviceType::Output);
    state
        .ensure_device_settings("speakers".into(), "Speakers".into(), DeviceType::Output)
        .communication_route = CommunicationRoute::SameDevice;
    (backend, state)
}

fn check(
    backend: &MockAudioBackend,
    state: &PersistentState,
    temporary: Option<&str>,
    main: &str,
    voice: &str,
) {
    let mut priorities = TemporaryPriorities::default();
    priorities.set(DeviceType::Output, temporary.map(Into::into));
    enforce_priorities(
        backend,
        state,
        &mut NotificationThrottler::new(),
        &priorities,
    );
    assert_eq!(backend.default_console.borrow()[&DeviceType::Output], main);
    assert_eq!(
        backend.default_multimedia.borrow()[&DeviceType::Output],
        main
    );
    assert_eq!(
        backend.default_communications.borrow()[&DeviceType::Output],
        voice
    );
}

#[test]
fn paired_output_and_unsplit_speakers_follow_temporary_switches() {
    let (backend, mut state) = setup(true);
    state.output.switch_communication_device = false;
    state.output.communication_priority_list = vec!["voice".into()];
    check(&backend, &state, None, "game", "voice");
    check(&backend, &state, Some("speakers"), "speakers", "speakers");
    check(&backend, &state, Some("game"), "game", "voice");
    check(&backend, &state, None, "game", "voice");
}

#[test]
fn unavailable_or_missing_pair_falls_back_without_erasing_configuration() {
    let (backend, mut state) = setup(false);
    check(&backend, &state, None, "game", "game");
    assert_eq!(
        state.devices["game"].communication_route,
        CommunicationRoute::Paired("voice".into())
    );
    state.devices.get_mut("game").unwrap().communication_route =
        CommunicationRoute::Paired("unplugged".into());
    check(&backend, &state, None, "game", "game");
}

#[test]
fn explicit_pairs_follow_windows_output_switches_without_a_priority_order() {
    let (backend, mut state) = setup(true);
    state.output.priority_list.clear();
    backend.set_default("game", DeviceType::Output);
    check(&backend, &state, None, "game", "voice");
    backend.set_default("speakers", DeviceType::Output);
    check(&backend, &state, None, "speakers", "speakers");
    backend.set_default("game", DeviceType::Output);
    check(&backend, &state, None, "game", "voice");
}

#[test]
fn paired_output_cannot_accidentally_route_to_an_input_endpoint() {
    let (mut backend, state) = setup(true);
    backend.devices[1].device_type = DeviceType::Input;
    check(&backend, &state, None, "game", "game");
}

#[test]
fn radio_power_off_falls_back_while_usb_endpoint_remains_active() {
    let (backend, mut state) = setup(true);
    state.devices.get_mut("game").unwrap().presence_source = PresenceSource::AstroA50X;
    check(&backend, &state, None, "game", "voice");
    *backend.presence.borrow_mut() = HeadsetPresence::Unavailable;
    check(&backend, &state, None, "speakers", "speakers");
    check(&backend, &state, Some("game"), "speakers", "speakers");
    *backend.presence.borrow_mut() = HeadsetPresence::Available;
    check(&backend, &state, None, "game", "voice");
    *backend.presence.borrow_mut() = HeadsetPresence::Unknown;
    check(&backend, &state, None, "game", "voice");
}

#[test]
fn radio_observation_does_not_affect_devices_without_opt_in() {
    let (backend, state) = setup(true);
    *backend.presence.borrow_mut() = HeadsetPresence::Unavailable;
    check(&backend, &state, None, "game", "voice");
}

#[test]
fn available_radio_does_not_revive_a_disabled_windows_endpoint() {
    let (mut backend, mut state) = setup(true);
    backend.devices[0].active = false;
    state.devices.get_mut("game").unwrap().presence_source = PresenceSource::AstroA50X;
    *backend.presence.borrow_mut() = HeadsetPresence::Available;
    check(&backend, &state, None, "speakers", "speakers");
}

#[test]
fn legacy_independent_communication_list_uses_presence_observations_too() {
    let (backend, mut state) = setup(true);
    state
        .devices
        .get_mut("speakers")
        .unwrap()
        .communication_route = CommunicationRoute::Inherited;
    state.devices.get_mut("voice").unwrap().presence_source = PresenceSource::AstroA50X;
    state.output.communication_priority_list = vec!["voice".into(), "speakers".into()];
    *backend.presence.borrow_mut() = HeadsetPresence::Unavailable;
    check(&backend, &state, Some("speakers"), "speakers", "speakers");
}

#[test]
fn simultaneous_migration_of_both_pair_endpoints_retains_the_link() {
    let (_, mut state) = setup(true);
    let backend = MockAudioBackend::new(vec![
        MockDevice::new("new_game", "A50 X Game", true),
        MockDevice::new("new_voice", "A50 X Voice", true),
        MockDevice::new("speakers", "Speakers", true),
    ]);
    state.devices.get_mut("game").unwrap().presence_source = PresenceSource::AstroA50X;
    assert!(migrate_device_ids(&backend, &mut state));
    assert_eq!(state.output.priority_list[0], "new_game");
    assert_eq!(
        state.devices["new_game"].communication_route,
        CommunicationRoute::Paired("new_voice".into())
    );
    assert_eq!(
        state.devices["new_game"].presence_source,
        PresenceSource::AstroA50X
    );
    check(&backend, &state, None, "new_game", "new_voice");
}

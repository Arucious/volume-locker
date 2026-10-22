//! Headset radio state is distinct from the USB audio endpoint's connection.
//! Unknown observations never override Windows' availability decision.

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HeadsetPresence {
    #[default]
    Unknown = 0,
    Available = 1,
    Unavailable = 2,
}

/// Observed on A50 X 046d:0b0b, firmware 1.3.21, during a real on/off cycle.
/// Ignore chatmix (command 0x0a): its center value also appears during power-off.
fn decode_a50x_report(report: &[u8]) -> Option<HeadsetPresence> {
    if !report.starts_with(&[0x02, 0x0c, 0x04, 0x00, 0x13, 0x00]) {
        return None;
    }
    match report.get(6)? {
        0x00 => Some(HeadsetPresence::Available),
        0x05 => Some(HeadsetPresence::Unavailable),
        _ => Some(HeadsetPresence::Unknown),
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{HeadsetPresence, decode_a50x_report};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    };
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    pub struct A50xMonitor {
        stop: Arc<AtomicBool>,
        state: Arc<AtomicU8>,
        worker: Option<JoinHandle<()>>,
    }

    impl A50xMonitor {
        pub fn start(on_change: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Self> {
            let stop = Arc::new(AtomicBool::new(false));
            let state = Arc::new(AtomicU8::new(HeadsetPresence::Unknown as u8));
            let worker_stop = Arc::clone(&stop);
            let worker_state = Arc::clone(&state);
            let worker = thread::Builder::new()
                .name("headset-presence".into())
                .spawn(move || {
                    let mut last_error = None;
                    while !worker_stop.load(Ordering::Relaxed) {
                        let result = listen(&worker_stop, &worker_state, &on_change);
                        // Losing the provider is not proof that the headset turned off.
                        publish(&worker_state, HeadsetPresence::Unknown, &on_change);
                        if let Err(error) = result {
                            let message = error.to_string();
                            if last_error.as_ref() != Some(&message) {
                                log::warn!("A50 X presence unavailable: {message}");
                                last_error = Some(message);
                            }
                        }
                        for _ in 0..10 {
                            if worker_stop.load(Ordering::Relaxed) {
                                break;
                            }
                            thread::sleep(Duration::from_millis(200));
                        }
                    }
                })?;
            Ok(Self {
                stop,
                state,
                worker: Some(worker),
            })
        }

        pub fn presence(&self) -> HeadsetPresence {
            match self.state.load(Ordering::Acquire) {
                1 => HeadsetPresence::Available,
                2 => HeadsetPresence::Unavailable,
                _ => HeadsetPresence::Unknown,
            }
        }
    }

    impl Drop for A50xMonitor {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    fn publish(state: &AtomicU8, next: HeadsetPresence, on_change: &Arc<dyn Fn() + Send + Sync>) {
        if state.swap(next as u8, Ordering::AcqRel) != next as u8 {
            log::info!("A50 X radio presence: {next:?}");
            on_change();
        }
    }

    fn listen(
        stop: &AtomicBool,
        state: &AtomicU8,
        on_change: &Arc<dyn Fn() + Send + Sync>,
    ) -> anyhow::Result<()> {
        let api = hidapi::HidApi::new()?;
        let mut candidates = api.device_list().filter(|device| {
            device.vendor_id() == 0x046d
                && device.product_id() == 0x0b0b
                && device.usage_page() == 0xff32
                && device.usage() == 0x0074
        });
        let candidate = candidates
            .next()
            .ok_or_else(|| anyhow::anyhow!("A50 X vendor HID interface not found"))?;
        anyhow::ensure!(
            candidates.next().is_none(),
            "Multiple A50 X headsets; automatic association is ambiguous"
        );
        let device = candidate.open_device(&api)?;
        log::info!("Listening for A50 X power events; initial state is unknown");
        // Passive input reports only: no HID output/feature writes or G HUB dependency.
        let mut report = [0_u8; 64];
        while !stop.load(Ordering::Relaxed) {
            let count = device.read_timeout(&mut report, 500)?;
            if count > 0 {
                log::debug!("Received A50 X HID input report ({count} bytes)");
            }
            if let Some(next) = decode_a50x_report(&report[..count]) {
                publish(state, next, on_change);
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub use windows::A50xMonitor;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_a50x_power_transitions_are_distinct() {
        assert_eq!(
            decode_a50x_report(&[2, 12, 4, 0, 19, 0, 0]),
            Some(HeadsetPresence::Available)
        );
        assert_eq!(
            decode_a50x_report(&[2, 12, 4, 0, 19, 0, 5]),
            Some(HeadsetPresence::Unavailable)
        );
    }

    #[test]
    fn unrelated_reports_and_timeouts_do_not_infer_power_off() {
        for report in [
            vec![],
            vec![2, 12, 4, 0, 19, 0],
            vec![2, 12, 4, 0, 10, 0, 6],
            vec![2, 12, 6, 0, 6, 0, 90, 90, 0],
        ] {
            assert_eq!(decode_a50x_report(&report), None);
        }
        assert_eq!(
            decode_a50x_report(&[2, 12, 4, 0, 19, 0, 99]),
            Some(HeadsetPresence::Unknown)
        );
    }
}

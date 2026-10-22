# Paired outputs and headset power: branch exploration

Branch: `codex/paired-outputs-headset-presence`, based on the fork's v0.9.0 source.

## Prototype behavior

Configure each existing **Output devices > device > Voice output** submenu:

| Main output | Voice output setting | Result |
| --- | --- | --- |
| A50 X Game | A50 X Voice | Games/media use Game; communication apps can use Voice |
| Speakers | Same as this output | Both roles use the speakers |
| Any other output | Use global voice settings | Preserve v0.9.0's global checkbox/independent list behavior |

This is a destination attached to an output, not another ordered list. The regular priority order and **Temporary default device priority** select the main output; its voice destination follows automatically. Explicit routes supersede the global communication list and checkbox for that selected output only. If the paired endpoint is missing, inactive, or explicitly unavailable, voice falls back to the selected main output. The pair remains saved so reconnection restores it.

With an empty main output priority list, explicit routes also follow manual Windows output changes without overriding the selected main output. With a configured priority list, Volume Locker continues enforcing that order; use its temporary output selector for manual overrides. Automatic power-off fallback requires another available output in that priority order.

The older independent communication priority lists remain available under **Default output/input device priority > Communication settings**. Input routing keeps its existing global policy. Existing configuration files load unchanged. Pair targets retain names for migration, and migrating both endpoints together preserves their link.

This changes Windows default roles; it does not force an app that explicitly selected a different endpoint to move. Discord must use a compatible default-device selection. Verify the particular client's behavior before relying on it.

## What we measured on the A50 X

Hardware: Logitech/Astro A50 X base, USB `046d:0b0b`, firmware `1.3.21`. Its vendor HID collection advertises usage page `ff32`, usage `0074`.

The user turned the headset off while keeping the powered base station connected:

| Observation | Headset on | Headset off |
| --- | --- | --- |
| Windows USB/PnP audio devices | Present, OK | Present, OK |
| Windows playback and recording endpoints | Present, OK | Present, OK |
| G HUB `/devices/list` | ACTIVE | ACTIVE |
| Passive HID command `0x13` payload | `00` | `05` |

The successful read-only input capture contained these seven-byte prefixes (reports are padded to 64 bytes):

```text
02 0c 04 00 13 00 00   headset power-on observation
02 0c 04 00 13 00 05   headset power-off observation
```

The capture also contained `02 0c 04 00 0a 00 06` during power-off. This is a chatmix command with a value that can occur during normal balance changes. The detector deliberately ignores it, along with battery, Bluetooth, volume, and unrelated reports. No raw device identifiers, serial numbers, or complete captures are included in this branch.

The final native Rust observer also confirmed the transitions with the base still connected: **Unavailable** after power-off and **Available** after power-on. This check used the same monitor implementation as the app, running in the read-only example. It did not change the user's live audio defaults. End-to-end automatic routing is covered by mocked integration tests; a full manual routing soak remains before a stable release.

The G HUB battery getter still returned a battery percentage while the headset was off. A cached battery value or a successful base-station request is not proof that the headset radio is available. G HUB's `lightspeed_connection` and `centurion/bool_states` paths returned `NO_SUCH_PATH` on this device. Neither is used by the prototype.

## Opt-in detector

Select **Output/Input devices > device > Availability > A50 X power events (experimental)** for each endpoint whose priority should depend on this radio. Keep **Windows connection** for speakers and other devices. Add speakers after A50 X Game in the normal output priority list to provide a fallback.

The Rust worker opens the matching vendor HID collection and reads input reports. It sends no output/feature reports and makes no G HUB calls. Recognized power changes trigger the existing device-change event and re-evaluate routing. Reads and reconnect attempts run off the event-loop thread. Stopping the detector joins its worker and closes its HID handle.

The state model is deliberately three-valued:

- **Unknown:** startup, provider failure/disconnection, or an unrecognized power value. Defer to Windows; this is not evidence of power-off.
- **Available:** a recognized on event. Windows must still report the endpoint active.
- **Unavailable:** a recognized off event. Skip opted-in endpoints during priority selection, including temporary choices and independent communication priorities.

Limitations before a stable release:

- Passive startup cannot determine an already-off headset's state. Toggle power once after enabling/restarting the monitor. An authoritative current-state query is still research work.
- These report meanings are validated against this A50 X firmware, not every Logitech headset. Other models need their own protocol adapters.
- Docking, sleeping, out-of-range behavior, and operation with G HUB fully exited need separate captures; they are not assumed equivalent to the observed power-off event.
- Only one A50 X is supported. Initial discovery refuses multiple matching HID interfaces rather than guessing an association.
- This is an exploratory branch; v0.9.0 and its release assets remain unchanged.

## Other devices

There is no universal USB-receiver flag for a remote headset's power. The audio stack can expose the receiver as active indefinitely. For another device, prefer its driver's connection/jack notifications if they track actual radio availability; otherwise investigate documented vendor telemetry or a device-specific HID adapter. Silence, battery level, mute state, and an idle audio stream are not reliable power signals. Timeouts and unsupported protocols must remain unknown.

The `PresenceSource` setting and backend presence query keep routing separate from provider-specific observations. A future provider can serve the same per-device **Availability** submenu without adding another priority list. The current implementation supports Windows availability and the experimental A50 X decoder only; it does not claim arbitrary-headset detection.

## Reproduce without changing audio settings

```powershell
# Observe the native detector for up to five minutes; Ctrl+C ends the observer.
cargo run --locked --example headset_presence -- 300

# Compare Windows and G HUB's views at different physical power states.
./scripts/Inspect-HeadsetPresence.ps1 -Label headset-on
./scripts/Inspect-HeadsetPresence.ps1 -Label headset-off
```

`scripts/Capture-HidReports.py` can record passive Windows HID input for a specific headset vendor interface. Use `--help` for arguments. `scripts/GHub.ReadOnly.psm1` exposes GET-only requests for investigation. Local snapshots belong in ignored `target/presence-research/`.

Automated coverage exercises split/unsplit temporary switches, unavailable pairs, endpoint type validation, endpoint migration, configuration compatibility and cleanup, opt-in power fallback, unknown observations, disabled Windows endpoints, and false-positive report rejection. The observer itself never selects Windows default audio endpoints.

## Reference material

- [Logitech A50 X guide, PC section](https://www.logitech.com/assets/70086/4/headphone_150x150.pdf#page=23): Game/Voice endpoints and the base station remaining available when docked.
- [Microsoft: device state constants](https://learn.microsoft.com/en-us/windows/win32/coreaudio/device-state-xxx-constants): endpoint state concerns the driver's Windows-visible device.
- [Microsoft: communication devices](https://learn.microsoft.com/en-us/windows/win32/coreaudio/using-the-communication-device): separate multimedia/communication roles.
- [A50 X HID exploration](https://github.com/alkitect/astro-a50x-media-pause/blob/main/docs/architecture/ADR-002-a50x-hid-dock-and-soft-power.md): a useful lead for passive power events; this branch uses the separately observed command `0x13` values instead of treating chatmix as power-off.
- [LGBattery protocol notes](https://github.com/bmrussell/LGBattery#lghub-api): read-only G HUB WebSocket resource discovery.
- [hidapi Rust API](https://docs.rs/hidapi/2.6.7/hidapi/): the Windows-native HID reader used by the prototype.

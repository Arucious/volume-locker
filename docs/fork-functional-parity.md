# Functional comparison with the fork

The combined `feat/upstream-feature-integration` branch was compared with the
fork's `feat/paired-outputs-headset-presence` at commit `da26e4c` on Windows.

## Menu and action comparison

The same probe was compiled against both checkouts. It built real native menu
objects through `rebuild_tray_menu`, recursively captured every label, separator,
submenu, enabled state, checkbox state and registered action, then compared the
results without the generated menu IDs. Action sequences used those generated
menu IDs to invoke the real handlers. Configuration was saved to JSON and loaded
again after each action.

All **27 menu/action scenarios matched exactly**, including:

- Fresh configuration and the existing volume/unmute locks and notifications.
- Output and input main/communication priority controls, including reordering
  and removal, disabled preferences and the Communication settings submenu.
- Per-output paired destinations, Same as this output, inherited routes and
  unavailable saved destinations.
- Input/output Availability menus, experimental-source selection and the
  reminder to toggle headset power.
- Output/input temporary overrides and clearing them.
- The update-available footer and notification preferences.

The standalone communication-priority PR initially retained an older, flat menu
layout. It now uses the fork's Communication settings submenu, with a regression
test checking both input/output and independent/mirrored communication modes.

## Routing comparison

All **864 routing combinations produced identical Windows role selections** in
the mocked backend. The matrix varied explicit/inherited output routes, presence
of a main priority order, independent communication priorities, the communication
checkbox, temporary selections, Unknown/Available/Unavailable observations, and
voice endpoints that were active, inactive, absent or the wrong endpoint type.
Each case also resolved input and input-communication priorities.

The combined build retains the fork's paired-output/presence event-handler and
configuration regression tests, alongside the isolated-feature tests. Input/output
selection, migration, stored routes and availability settings are preserved.

## Deliberate differences

- Repository, download and update targets point to `felipecrs/volume-locker` for
  upstream contribution. The fork's releases and original branches remain intact.
- Console and Multimedia defaults are checked independently, fixing Multimedia
  drift when Console is already correct. Restore notifications require a
  successful switch. These are intentional corrections to the fork's resolver.
- The HID reader accepts a protocol profile rather than being hard-coded to
  A50 X. Its actual A50 X identity, report decoder, passive reads, opt-in behavior
  and Unknown-state policy are retained. Other physical models remain unsupported
  until their profiles are implemented and validated.

The individual PRs deliberately contain only their respective features. Full
menu parity refers to their combined integration. The comparison checked native
menu data and mocked routing, not an interactive Windows audio routing soak.
It did not change live audio defaults. Physical startup/docking/sleep/range tests
remain outstanding for experimental HID support.

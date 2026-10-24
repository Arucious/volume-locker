# Upstream feature review and merge order

The fork's combined changes are split into three independently buildable PRs
against `felipecrs/volume-locker:master`, based on upstream commit `34784db`.

| Order | Pull request | Branch | Scope | State |
| --- | --- | --- | --- | --- |
| 1 | [#176](https://github.com/felipecrs/volume-locker/pull/176) | `feat/communication-priorities` | Independent communication priorities for input and output | Draft |
| 2 | [#177](https://github.com/felipecrs/volume-locker/pull/177) | `feat/output-voice-routing` | Voice destinations attached to individual outputs | Draft |
| 3 | [#178](https://github.com/felipecrs/volume-locker/pull/178) | `feat/experimental-hid-presence` | Opt-in A50 X availability and reusable passive HID profiles | Draft |

These are separate upstream PRs with a suggested merge order, not a native GitHub
stack. Download links, package repository metadata and the in-app updater continue
to point to the original repository in every PR. The earlier fork feature branches
and releases are preserved.

## Review the pieces

1. Review communication priorities first: Console/Multimedia and Communications
   can resolve independently for input and output. Empty communication lists keep
   the existing checkbox behavior. Temporary selections affect the main list.
2. Review output routes second: explicit SameDevice/Paired destinations follow
   normal and temporary output choices, or manual Windows switches when no main
   priority order exists. Explicit routes override global communication settings;
   inherited routes retain the independent lists from the first feature.
3. Review availability separately: Windows availability remains the default.
   Confirmed radio-off observations filter opted-in endpoints from every resolver.
   Unknown observations defer to Windows. Only the A50 X has a real protocol
   implementation; the second protocol in tests is synthetic.

## Combined integration and overlapping files

The PRs intentionally build without each other, but their resolver, configuration
and menu changes overlap. Later PRs may need conflict resolution after earlier PRs
merge. The branch
[`feat/upstream-feature-integration`](https://github.com/Arucious/volume-locker/tree/feat/upstream-feature-integration)
already resolves those overlaps in a linear sequence of three feature commits.
It also tests cross-feature precedence, unavailable paired endpoints, independent
communication fallback and simultaneous pair migration.

To inspect or build the combined result in a clean checkout:

```sh
git fetch https://github.com/Arucious/volume-locker.git feat/upstream-feature-integration
git switch --detach FETCH_HEAD
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --frozen
```

The prepared integration commits are `dd66a8e` (communication priorities),
`407482d` (output routes on top of that feature), and `905fcad`
(HID availability on top of both). Follow-up commits add validation and documentation.
These feature commits are useful starting points when updating
the remaining PRs after upstream merges. Apply only layers that have not already
landed. If upstream changes further, rebase and rerun checks.

## Validation

Local checks ran on Windows with Rust 1.98.1. Every row passed formatting, Clippy
for all targets with warnings denied, and a frozen release build.

| Checked version | Unit tests | Windows bindings | Example tests | Total |
| --- | ---: | ---: | ---: | ---: |
| Communication priorities alone | 129 | 1 | 0 | 130 |
| Output routes alone | 118 | 1 | 0 | 119 |
| HID availability alone | 120 | 1 | 8 | 129 |
| Communication priorities + output routes | 138 | 1 | 0 | 139 |
| All three integrated | 157 | 1 | 8 | 166 |

The diagnostic Python source and both PowerShell scripts parse successfully.
A two-second read-only observer run opened the matching A50 X HID interface and
shut down cleanly. It remained Unknown because no power transition was observed.
Automated tests use mocked audio backends and captured report fixtures; no live
Windows default devices were changed during validation.

A direct comparison with the original fork matched all 27 native-menu/action
scenarios and all 864 routing combinations. The standalone communication menu
was aligned with the current fork layout and the original cross-feature tests
were retained. See [the functional parity report](fork-functional-parity.md).

The HID draft still needs physical on/off routing and a soak test, startup-state
research, and captures for docking, sleep, out-of-range and G HUB-exited behavior.
See [the experimental availability notes](experimental-headset-presence.md).

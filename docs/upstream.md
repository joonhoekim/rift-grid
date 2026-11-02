# Upstream notes

Where the fork meets upstream rift, for merging `upstream/main` and for
contributing back. The fork keeps upstream's code as it is unless upstream has a
defect the fork cannot live with; what follows is noted here rather than
changed.

## Hooks in upstream files

New code lives in fork modules; upstream files only call into them.

| Upstream file | What the fork adds |
| --- | --- |
| `actor/reactor.rs` | Three `Event` variants (`BorderSettle`, `WindowsReordered`, `GridMotion`); in `handle_event_inner`, the border, grid motion and grid display handlers, `grid_motion_before`/`after`, `settle_display_grids` after a display change, and `update_borders`; `save_cells` last in `handle_event_inner` |
| `actor/reactor/query.rs`, `model/server.rs` | The grid fields of `RuntimeWorkspaceData` (`grid_cell`, `grid`, `grid_elsewhere`, `layout_focus`) |
| `actor/reactor/events/command.rs` | The list of grid commands and their arm in the command-kind `match`, and switch tracking for commands that change cells only when they cross an edge |
| `layout_engine/engine.rs` | Grid commands in `handle_command` and `handle_virtual_workspace_command`; the recent window list; the calls into `engine/departures.rs` (see below); `ready_remembered_cell` before both checks for a departed window's workspace, and `put_back_layouts` first on `SpaceExposed` (`engine/cells.rs`) |
| `model/virtual_workspace.rs` | `grid_current`; `shown_before`, which `ensure_space_initialized` shows first; the settings kept as `grid::effective_settings` (a workspace for every cell, named by grid and cell) in `new_with_config`, `plan_settings_names` and `apply_settings`; `remember_grid_cell` in `set_active_workspace`; `remap_grid_cells` in `remap_space`; app rules' `grid` through `rule_workspace` in `apply_app_rule_decision` |
| `common/config/types.rs` | `grid_columns`, app rules' `grid`, `validate_grid`; grid fields in `MissionControlSettings`, whose `Default` is written out by hand |
| `ui/mission_control.rs`, `actor/mission_control.rs` | The grid overview's two stages, gliding cards, and refresh on reactor broadcasts; `set_capture_limits` first in `OverviewSession::new`; in `sync_metal`, the size previews are captured at (`card_capture_size`), a grid card's preview frame (`card_image_frame`), the focused window's border (`preview_border`) and the canvas resized with its cell (`fit_canvas`); the grid buttons' input first in `OverviewSession::input` (`grid_bar_input`) |
| `ui/menu_bar.rs`, `actor/menu_bar.rs` | Grid maps and the grid entries of the Workspace menu; the grid fields in the settings preview's `Workspace` |
| `actor/gesture.rs` | `ActionConfig::grid`; the grid's actions in `Settings` (first in `action_for`, and `consume` while they are on); `DeviceSession` keeps `settings` and `mode` in place of upstream's `alternate`, so a late finger can pick any action; a `Lifecycle::Grid` variant, and one call to `grid_swipe` for a grid action's swipe frames |
| `sys/gesture.rs`, `actor/input.rs` | `Filter::vertical`: with a vertical grid swipe on, vertical Dock swipes (Mission Control, App Exposé) go through arbitration too |
| `actor/input.rs` (gesture `configure`) | Gestures stay on while the Overview is open when the grid's are; `Control::set_overview` after `configure`, and `action_for` then gives only grid actions |
| `actor/wm_controller.rs`, `actor/mission_control.rs` | `WmEvent::OverviewStep` passed on as `mission_control::Event::Step`, handled by `overview_step` |
| `actor/reactor/gesture.rs` | `Lifecycle::Grid` goes to `grid_gestures.rs`; `gesture_space_active` is `pub(super)` |
| `actor/wm_controller.rs` | The Overview closes on a space state update only when `grid_motion::screens_changed` says the displays or their Spaces changed |
| `sys/mach.rs`, `sys/service.rs`, `common/config/types.rs`, `crates/rift-client/src/lib.rs`, `Cargo.toml`, `assets/Info.plist`, `rift.default.toml` | The fork's names (`crates/rift-protocol/src/names.rs`): the Mach service and launchd label, the binary looked up on `PATH`, the log paths, the config and data directories, the default layout folder, the `[[bin]]` names, the bundle id; clap's `name` in both binaries |
| `bin/rift.rs` | `load_cells` after the layout engine is made |
| `Cargo.toml` | `objc2-screen-capture-kit`, which upstream dropped with its screenshot previews: the grid's slides capture windows with it |
| `ui/settings/*` | A Window Borders category (also in `sidebar.rs`'s color match), the fork's Overview page in place of `interface::overview` (`screen_recording_status` made `pub(super)` for it), and their search entries; in `mod.rs`, one call that ends a field's editing on a click elsewhere or when the window goes to the back (`settings/editing.rs`), checked in `native_tests.rs` |

## cgs

`cgs/` is cgs as published on crates.io, used through `[patch.crates-io]` in
`Cargo.toml` (upstream keeps a `cgs` directory out of the workspace for the
same). One fork commit changes it: the capture memory limits, constants in
`src/compositor/preview/capture.rs`, can be set with `cgs::set_capture_limits`
(full-resolution previews need more). When upstream takes a newer cgs:

1. Replace `cgs/` with the new version from `~/.cargo/registry/src/*/cgs-<ver>`,
   without its `Cargo.lock`, as a commit of its own.
2. Put the limits change on again (`git show` the earlier `feat(cgs)` commit).

## Where merges conflict most

These could shrink the fork's footprint in upstream files. None is a defect.

- The grid commands are listed in three places: `command.rs` and twice in
  `engine.rs`. A `LayoutCommand::is_grid()` in the protocol crate would leave
  one.
- `MissionControlSettings` lost its derived `Default`, so every field upstream
  adds must also go into the fork's impl. Moving the grid fields to their own
  `[settings.ui.grid_overview]` table would restore the derive, at the cost of a
  config change.
- The settings app's category list, page match and search catalog: upstream
  reorganizes these often, and each time the fork's Overview and Window Borders
  entries conflict. `grid_pages::overview` copies upstream's Overview sections,
  so check it against `interface::overview` after a merge.
- `reactor.rs` has seven separate hook calls. One `Event::Fork` variant with a
  hook before and after handling would make it two.

## Drag and drop: replaced rather than hooked

Here the fork does depart from the rule above. Upstream's targeting for dragging
a tile into place had grown layers (stickiness at zone edges, the nearest tile
across gaps, keyboard-style moves off the dragged tile's own edges, retrying the
next candidate when the layout refused one, turning an insert that kept its slot
into the center action), and the fork replaced it with the model of VS Code's
editor groups, in every layout:

- Only the tile under the pointer is a target, in one of five zones
  (`classify_zone`): an edge inserts on that side, the middle swaps (always;
  `drag_drop.drop_action` is still parsed but nothing reads it). A tile owns its
  left and top edges only. Over the dragged window's own tile there is no
  target, only the home preview. The targeting is `actor/drag/dock.rs`.
- The preview is where the window will land: `drop_preview_frame` runs every
  action, Swap included, on a copy of the layout. A refused drop shows nothing
  and is not asked again while the pointer stays in that zone
  (`TargetState::Rejected`).
- The Overview uses the same targeting (`hit_test_projected`). It swaps only
  within a cell, outlining the tile; the middle of a tile in another cell moves
  the window into that cell, as the cell's empty space does. A drop that keeps
  the window's slot is taken, as on the screen.

Upstream's code for all this is deleted, not bypassed, in `actor/drag.rs`,
`model/drag.rs`, `actor/reactor.rs` (`resolve_drag_preview`,
`refresh_active_drag_scene`, which builds the scene for the space the drag began
in), `layout_engine/engine.rs` (`DropPreview`, `source_move_neighbors`, the
Overview's same-slot check) and `ui/mission_control.rs` (`move_drag`). When a
merge conflicts there, keep the fork's side and carry over only what upstream
fixed. `WindowDropAction::Move` is no longer produced by a drag; the layouts
still handle it.

Left as upstream has them: in the scrolling layout a swap carries each window's
height weight into the other column, so after a vertical resize a window can
come out 0 px tall (the preview now shows it); master_stack takes inserts that
its next normalization undoes.

## Upstream behavior that meets the grid

- `next_workspace`/`prev_workspace`, and the trackpad swipes that use them, walk
  all 81 workspaces in a line, so they cross from one row or grid into the next.
- The grid's swipes take their finger counts from upstream's workspace and
  Scrolling layout swipes while the grid is on: `[settings.gestures]` still
  gives them their distance, vertical tolerance and haptics.
- `cargo test --lib` aborts now and then (about 1 run in 10 to 20, on macOS 27)
  with `Cannot form weak reference to instance ... of class
  SLSWindowManagementFallbackBridge` and SIGABRT, before and after the fork's
  2026-10-20 changes alike: a SkyLight crash while tests touch the
  WindowServer, not a failing test. Run it again.
- `move_workspace_to_display` reorders workspaces, while the grid takes a
  workspace's grid and cell from its position. Not yet checked with grids on.
- `VirtualWorkspaceSettings::validate` checks `default_workspace` and
  `workspace_names` against `default_workspace_count`, which grids ignore: they
  have `9 × grid_columns²` workspaces and name cells per grid. The store keeps
  the grid's own count and names (`grid::effective_settings`), so upstream's
  naming and repair code sees them as configured.
- `activate_workspace` reports the space it shows in `workspace_space`, and the
  reactor then makes that the command display. Handing a grid to another
  display clears it, so the focus stays where it was.
- The workspace mode `global` (#603) puts each workspace on one display at a
  time, as grids are under `grid_displays.rs`. The grid has not been tried with
  it; it is meant for `per_display`, the default.
- `named_pin_reload_moves_immediately_without_growing_catalog` fails on
  upstream (`3114cc5`, `adca720`) on a Mac with windows open, and passes on
  upstream's CI. With no override, `try_space_window_list_for_connection` asks
  the real WindowServer, and the tests' Space 1 is a real desktop: its window
  list holds the machine's windows and not the test's, so the test's window is
  marked hidden after its first move and `can_place_window` refuses the second.
  Neither the toolchain (1.96, nightly 1.101), nextest's one process per test,
  nor the cursor changes it. The fork makes the function never ask the
  WindowServer in tests (an empty list unless a test sets one), in a commit of
  its own (`cba2ab5`) to send upstream as is; that also covers the case below.
- Upstream is formatted with nightly rustfmt (`rustfmt.toml` sets unstable
  options). Stable `cargo fmt` rewrites much of upstream, so format the fork's
  own files only, with nightly when available.
- `empty_active_space_membership_during_wake_race_does_not_blank_known_active_windows`
  fails on upstream on a Mac with windows open. The test fixture
  `forwarded_space_state` asks the real WindowServer which windows are on space
  1, which is a real desktop, so the "empty" membership the test means to
  simulate is not empty. The fork sets the test's empty window-list override
  before the setup call instead of after it, in a commit of its own, to send
  upstream as is. If upstream fixes it another way, take upstream's version in
  the merge.
- On macOS 27, `settings_native --verify` fails on upstream too: switching to
  Traditional at a 700×420 window widens it to 704. The other checks pass with
  that size left out.
- On macOS 27 the unit tests abort now and then (about one run in five, mostly
  the first after a build) with `Cannot form weak reference to instance of
  class SLSWindowManagementFallbackBridge`, inside SkyLight. Run them again.

- Settings text fields save when editing ends, which AppKit does on Return or
  Tab only: a value typed and left there is saved when the page changes or
  the window closes. The fork's `settings/editing.rs` also ends it on a click
  elsewhere in the window and when the window goes to the back, and could go
  upstream as is.

- A window minimized, hidden with its app or put in native fullscreen came
  back next to whatever was selected, at default sizes, and a hidden app's
  windows could come back in the workspace on screen instead of their own
  (upstream #520, with #479, #511 and #541). The fork's
  `layout_engine/engine/departures.rs` keeps the workspace's layouts from before
  the window left and puts them back when nothing else changed meanwhile, or
  else puts the window after its old neighbor; it works for every layout mode.
  Sent upstream as #595 (from the `joonhoekim/rift` fork, remote `pr`).

## App lifecycle fixes to send

Fixes of upstream code, each its own commit so a pull request can take it
as is. Each was reproduced before the fix; the first two were also checked
live after it.

- `c165ce1` *drop apps that quit, whose pid Carbon can no longer read*:
  `kEventAppTerminated` arrives after `GetProcessPID` stops resolving the
  process, so upstream never terminates an app; every quit app stays in
  `rift-grid-cli query applications` with its actor and AX observer.
- `c9eba46` *manage an app whose window shows before it turns regular*: an app
  started outside LaunchServices (an exec binding) reads Prohibited until it
  sets its policy, and when both discoveries read Prohibited its windows are
  never managed. Reproduced with a small app that shows a window and turns
  Regular three seconds later.
- `66eb1af` *attach again a running app whose actor gave up*: an app busy when its
  actor starts (seen with Chrome and Brave as rift restarted) refuses AX
  requests for the two seconds `init` waits, the actor exits, and only
  activating the app attaches it again. Its windows stay where the last rift
  parked them, so nothing shows it to activate. `AppLifecycle::sync` now
  matches actors to the running apps in both directions (the existing check
  for quit apps is its other half), run on app activation and deactivation and
  on space state updates, and after a failure with a retry that doubles from 2
  seconds to 5 minutes. With tests in `wm_controller.rs`
  (`app_lifecycle_tests`) that do not build without the fix.

## Display move fixes to send

Two fixes of #601's revisioned space state, each its own commit. Before them,
`move_window_to_display` (Alt+Ctrl+IJKL) left the window at its old size or
place on the new display until the next layout. Both were reproduced with
trace logs and checked live after the fix.

- `994c7d5` *keep the frames in flight when a window only moves between
  Spaces*: the window's own change of Space bumped the revision and revoked the
  resize sent a moment before. With a test in `spaces/tests.rs` that fails
  without the fix.
- `89385be` *don't end animations on a space snapshot that revoked nothing*:
  the repeat of the accepted revision about 100 ms later stopped every
  animation where it was; seen with Chrome, whose resizes are slow.

## Gesture changes to propose

The grid's trackpad swipes hook into upstream's gesture code in several
places (see the table above). These upstream changes would remove most of
those hooks, and are useful upstream on their own. None has been proposed yet.

- **Swipes bound to commands, by finger count and direction.** Upstream's
  gestures are two fixed actions: horizontal workspace switching and the
  Scrolling layout strip. A table like `[gestures] three.left = "..."` (as in
  subline) binding any command to three or four fingers in each direction
  would make the cell swipe plain configuration (`grid_cell`), leaving the
  fork only its grid and Overview commands. It needs two changes the fork
  already carries: vertical Dock swipes (Mission Control, App Exposé) going
  through the event tap's arbitration (`Filter::vertical`), and a late finger
  picking any configured action (`action_for(mode, count)` in place of the
  two-action `alternate`). Worth sending as a feature pull request.
- **Gestures while the Overview is open.** Upstream turns gesture recognition
  off while the Overview is open, so with `consume_dock_swipe = true` a
  three-finger swipe over the Overview still switches macOS Spaces, and four
  fingers up open Mission Control. Recognizing the swipes there, at least to
  keep them from macOS, is a bug fix of its own. Fix it in the fork first and
  send the same commit, as with the test fixes.
- **A command to step the Overview's stages.** The reactor has no way to the
  Overview actor, so the fork added `WmEvent::OverviewStep`. A command for
  stepping out and in would serve keys, the CLI and gestures alike. The stages
  themselves (Esc stepping) are the fork's, so this waits
  for the first item.

## Contributing back

See [FORK.md](../FORK.md#upstream-candidates): each candidate goes as a pull
request from a fresh branch off `upstream/main`.

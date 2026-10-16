# Upstream notes

Where the fork meets upstream rift, for merging `upstream/main` and for
contributing back. The fork keeps upstream's code as it is unless upstream has a
defect the fork cannot live with; what follows is noted here rather than
changed.

## Hooks in upstream files

New code lives in fork modules; upstream files only call into them.

| Upstream file | What the fork adds |
| --- | --- |
| `actor/reactor.rs` | Three `Event` variants (`BorderSettle`, `WindowsReordered`, `GridMotion`); in `handle_event_inner`, the border and grid motion handlers, `grid_motion_before`/`after` and `update_borders` |
| `actor/reactor/events/command.rs` | The list of grid commands, and switch tracking for commands that change cells only when they cross an edge |
| `layout_engine/engine.rs` | Grid commands in `handle_command` and `handle_virtual_workspace_command`; the recent window list |
| `model/virtual_workspace.rs` | `grid_columns`, `grid_current`; `workspace_count`; `remap_grid_cells` in `remap_space`; a `grid` argument to `resolve_rule_workspace_with_policy` |
| `common/config/types.rs` | `grid_columns`, app rules' `grid`, `validate_grid`; grid fields in `MissionControlSettings`, whose `Default` is written out by hand |
| `ui/mission_control.rs`, `actor/mission_control.rs` | The grid overview's two stages, gliding cards, and refresh on reactor broadcasts |
| `ui/menu_bar.rs`, `actor/menu_bar.rs` | Grid maps and the grid entries of the Workspace menu; the grid fields in the settings preview's `Workspace` |
| `ui/settings/*` | A Window Borders category (also in `sidebar.rs`'s color match), the fork's Overview page in place of `interface::overview`, and their search entries |

## Where merges conflict most

These could shrink the fork's footprint in upstream files. None is a defect.

- The grid commands are listed in three places: `command.rs` and twice in
  `engine.rs`. A `LayoutCommand::is_grid()` in the protocol crate would leave
  one.
- `command.rs` puts `is_grid_command ||` before upstream's `matches!`, which
  re-indents upstream's lines; putting it after would not.
- `MissionControlSettings` lost its derived `Default`, so every field upstream
  adds must also go into the fork's impl. Moving the grid fields to their own
  `[settings.ui.grid_overview]` table would restore the derive, at the cost of a
  config change.
- `resolve_rule_workspace_with_policy` takes an extra `grid` argument. Resolving
  the grid at its one call site would leave upstream's signature alone.
- The settings app's category list, page match and search catalog: upstream
  reorganizes these often, and each time the fork's Overview and Window Borders
  entries conflict. `grid_pages::overview` copies upstream's Overview sections,
  so check it against `interface::overview` after a merge.
- `reactor.rs` has five separate hook calls. One `Event::Fork` variant with a
  hook before and after handling would make it two.

## Upstream behavior that meets the grid

- `next_workspace`/`prev_workspace`, and the trackpad swipes that use them, walk
  all 81 workspaces in a line, so they cross from one row or grid into the next.
- `move_workspace_to_display` reorders workspaces, while the grid takes a
  workspace's grid and cell from its position. Not yet checked with grids on.
- `VirtualWorkspaceSettings::validate` checks `default_workspace` and
  `workspace_names` against `default_workspace_count`, which grids ignore: they
  have `9 × grid_columns²` workspaces and name cells per grid.
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

## Contributing back

See [FORK.md](../FORK.md#upstream-candidates): each candidate goes as a pull
request from a fresh branch off `upstream/main`.

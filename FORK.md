# rift-grid

A fork of [rift](https://github.com/acsandmann/rift) that lays the virtual
workspaces out as a grid, as [subline](https://github.com/joonhoekim/subline)
does on Linux, and draws window borders in place of JankyBorders. Branch
`grid`; `main` mirrors upstream.

## What the fork adds

| Feature | Where | Settings |
| --- | --- | --- |
| Grids of cells (see [docs/grids.md](docs/grids.md)): `switch_to_grid`, `move_window_to_grid`, `grid_focus`, `grid_move`, `grid_cell`, `grid_center`, `focus_recent_window`, app rules' `grid` | `layout_engine/engine/grid.rs`, `model/virtual_workspace/grid.rs`; topology from subline's `subline-grid` | `[virtual_workspaces] grid_columns` |
| Grid overview: two stages, follows the real focus, live previews, labels in the gaps | `ui/mission_control/grid.rs` | `[settings.ui.mission_control] grid_*`, `live_preview_interval_ms` |
| Menu bar 3x3 maps, one per grid in use | `ui/menu_bar/grid_map.rs` | `[settings.ui.menu_bar] grid_maps` |
| Window borders: sliding focused border, glow, hidden under the windows above | `ui/borders.rs`, `actor/reactor/borders.rs` | `[settings.ui.borders]`, `common/config/borders.rs` |
| Settings app pages | `ui/settings/grid_pages.rs` | — |

Upstream's own files keep only the lines that call into these modules. Keep it
that way: new fork code goes in a module of its own, and a change inside an
upstream file is a hook of a few lines.

## Taking upstream

Merge rather than rebase, so `grid` never needs a force push:

    git fetch upstream
    git merge upstream/main
    cargo test --lib

One test, `empty_active_space_membership_during_wake_race_does_not_blank_known_active_windows`,
fails on upstream too. Install with `apps/rift-grid` in nixos-config, which
builds, signs with the machine's local identity and restarts rift.

subline's crates are a private git dependency pinned by `rev` in `Cargo.toml`;
cargo fetches them with the git CLI (`.cargo/config.toml`), so `gh auth` must be
set up. Move the `rev` forward to take subline's changes.

## macOS facts the borders depend on

Found by experiment on macOS 26/27; recheck when macOS changes:

- A window cannot be ordered just above another app's window. `SLSOrderWindow`,
  `SLSTransactionOrderWindow` and ordering groups all put it above every app's
  windows. Borders therefore sit above everything and mask out what the windows
  in front of their window cover.
- Moving a WindowServer window and changing its layers reach the screen
  separately, so a border never moves its window while its layers move: a slide
  runs on a separate overlay.
- A border surface is the size of its window at the display's scale; a whole
  display's overlay costs tens of MB.
- The frontmost app's main window passes through that app's previous main window
  when focus moves to another app's window; the borders bridge that with the
  window rift is focusing for up to 600 ms.

## Upstream candidates

Once these have run daily for a while, they could go upstream as pull requests,
each from a fresh branch off `upstream/main`: window borders (no JankyBorders
needed), the workspace grid with its overview and menu bar map, and live
overview previews.

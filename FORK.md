# rift-grid

A fork of [rift](https://github.com/acsandmann/rift) that lays the virtual
workspaces out as a grid and draws window borders in place of JankyBorders. Branch
`grid`; `main` mirrors upstream.

https://github.com/user-attachments/assets/93fb8b0f-f88f-4140-a179-30bf3c8bd829

## Why

rift-grid started from subline, a Wayland compositor for Linux by the same
author, which is not public. The aim was to work on macOS the way subline works
on Linux. rift was already the window manager of choice there, so this is a fork
of rift that builds on what rift does well.

What subline set out to do, and this fork does on rift:

- Place workspaces in a spatial model, a grid of cells, so that each cell is
  easy to remember and to reach.
- Name a cell by where it is, not by a number: the center, the top left, the
  left, the bottom left, or as an offset from -1 to 1 on each axis. A place in a
  square is easier to picture than a number from 1 to 9.
- Move by direction: going from the center to the bottom right is more natural
  than going from 1 to 9.
- Show the whole n x n grid from above in the overview, rather than workspaces
  in one row or wrapped into rows.

## What the fork adds

| Feature | Where | Settings |
| --- | --- | --- |
| Grids of cells (see [docs/grids.md](docs/grids.md)): `grid`, `send_to_grid`, `focus`, `move_tile`, `go_to_cell`, `focus_default_cell`, `focus_recent_window`, `overview_whole`, app rules' `grid` | `layout_engine/engine/grid.rs`, `model/virtual_workspace/grid.rs`; topology in `common/grid_nav.rs` | `[settings.grid] rows`, `columns`, `start`, `names` |
| Grids belong to displays: a grid is on one display at a time, Alt+N goes there, each display comes up on a grid of its own, and `move_grid_to_display` hands one over (the overview's grid buttons, a row per display) | `actor/reactor/grid_displays.rs`, `ui/mission_control/grid_bar.rs` | — |
| Grid overview: two stages, follows the real focus, live previews, a label row on each tile | `ui/mission_control/grid.rs` | `[settings.grid.overview]` |
| Moves shown: the screen slides to the cell moved to, fades to another grid, and a small grid map shows each cell's tiles and rings the cell arrived at; the screen's edge lights up where a move finds no cell beyond; the slides and borders follow a motion level (Reduce motion by default) | `ui/grid_motion.rs`, `ui/grid_motion/edge.rs`, `actor/reactor/grid_motion.rs` | `[settings.grid.moves]`, `[settings.grid] motion` |
| Trackpad swipes as subline's table: an action for each swipe of three and four fingers (by default three to the neighboring cell, four through the display's grids and up for the Overview); taken before macOS and upstream's swipes | `actor/gesture/grid.rs`, `actor/reactor/grid_gestures.rs` | `[settings.grid.gestures]`, `common/config/grid_gestures.rs` |
| Focus stays in a cell its last window left, as in subline: rift-grid's invisible holder takes the keyboard there, in place of the desktop | `actor/reactor/cell_focus.rs`, `ui/home/holder.rs` | — |
| Menu bar grid maps, one per grid in use | `ui/menu_bar/grid_map.rs` | `[settings.grid] grid_map` |
| Window borders: sliding focused border, glow, hidden under the windows above | `ui/borders.rs`, `actor/reactor/borders.rs` | `[settings.grid.focus]`, `common/config/borders.rs` |
| Settings app in subline's order (Input, Workspace, Shell, System): Grid, Cells and Focus pages, and the grid's sections on the Overview, Mouse & Trackpad and Menu Bar pages | `ui/settings/grid_pages.rs`, `ui/settings/grid_sidebar.rs` | — |
| Drag and drop as in VS Code's editor groups: the tile under the pointer, five zones, a preview of where the window lands; replaces upstream's targeting (see [docs/upstream.md](docs/upstream.md)) | `actor/drag/dock.rs` | `[settings.drag_drop] drop_zone_fraction` |

subline features put off for now (a drawer, grid presets, a slide that follows
the fingers, carrying a tile over the screen's edge) are in
[docs/later.md](docs/later.md), with what is known about each on macOS.

The fork's binaries, Mach service, launchd label, bundle id and directories have
names of their own (`rift-grid`, `rift-grid-cli`, `io.github.joonhoekim.rift-grid`,
`~/.config/rift-grid`, `~/.rift-grid`), so it never talks to an upstream rift
or reads its files. They are kept in `crates/rift-protocol/src/names.rs`; the
user-visible "Rift" in menus and windows stays upstream's.
[docs/install.md](docs/install.md) has the side-by-side table.

Upstream's own files keep only the lines that call into these modules. Keep it
that way: new fork code goes in a module of its own, and a change inside an
upstream file is a hook of a few lines.

## Taking upstream

Merge rather than rebase, so `grid` never needs a force push:

    git fetch upstream
    git merge upstream/main
    cargo test --lib
    cargo test --test settings_native -- --ignored --verify

The second compiles the settings app's sources on their own and opens a hidden
settings window; `--lib` does not build it.

[docs/upstream.md](docs/upstream.md) lists where the fork hooks into upstream
files, where merges conflict most, and upstream behavior that meets the grid.

## After a macOS update

Unit tests cover logic and hardly touch macOS, so an update is checked in two
steps, each compared with the last run on the previous macOS version:

    tools/test-baseline    every test's result; lists the ones that changed
    tools/live-check       the running rift: event tap, border windows and their
                           order, windows moved off screen and back, the move
                           slide and grid map, the Overview's windows

Run both before updating to record the baseline, and again after. Results are
kept in `~/.local/state/rift-grid/`. A few upstream tests read the live
WindowServer and fail on some machines whatever the code; comparing results
rather than counting failures leaves them out. `live-check` takes over a
display for about 15 seconds (two empty cells side by side, two test windows);
keep hands off meanwhile. If something it checks changed, the facts below are
where to look first.

## macOS facts the borders depend on

Found by experiment on macOS 26/27; recheck when macOS changes:

- A window cannot be ordered just above another app's window. `SLSOrderWindow`,
  `SLSTransactionOrderWindow` and ordering groups all put it above every app's
  windows. Borders therefore sit above everything and mask out what the windows
  in front of their window cover.
- Moving a WindowServer window and changing its layers reach the screen
  separately, so a border never moves its window while its layers move: a slide
  runs on a separate overlay.
- A WindowServer window's surface costs its whole area at the display's scale
  (a 1671x1375 window at 2x is about 36 MB). A border is therefore four bands
  around its window, and the slider is made for each slide and dropped after.
- The no-shadow tag (bit 3) leaves a window its WindowServer shadow; it takes
  `CGSWindowSetShadowProperties` with `com.apple.WindowShadowDensity` 0. Without
  it each band's shadow falls on the next.
- A CoreAnimation shadow is drawn only from what lies in its window, so a glow
  split over bands fades at their ends; the glow is a nine-slice image instead.
- The frontmost app's main window passes through that app's previous main window
  when focus moves to another app's window; the borders bridge that with the
  window rift is focusing for up to 600 ms.

## Upstream candidates

Once these have run daily for a while, they could go upstream as pull requests,
each from a fresh branch off `upstream/main`: window borders (no JankyBorders
needed), the workspace grid with its overview and menu bar map, and live
overview previews. For gestures, docs/upstream.md lists changes to propose
that would shrink the fork's hooks, and for the settings app, the Keyboard
page's one row per command. A test fix, which the fork carries as its own commit so the
pull request can take it as is, can go any time; see
[docs/upstream.md](docs/upstream.md).

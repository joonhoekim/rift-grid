# Put off for later

subline features looked at for rift-grid and put off (2026-11-27), with what
is known so far, so the work can pick up from here. subline's references are
to its own repository (`docs/design/*.md`, `crates/compositor/src/*`).

## Drawer

subline's `docs/design/drawer.md`, `crates/compositor/src/drawer/`: a window
kept out of the grids. It slides out from a screen edge over the tiles when
opened. Closing hides it while the app keeps running. A tile can be put in a
drawer, and a drawer's window taken out into the current cell. A key toggles
the latest drawer (Mod+\`, Mod+Shift+\` to put in or take out). A drawer can
open the default terminal, file manager or browser, and drawers come back
with the session.

On macOS:

- **Staying on top**: subline keeps the drawer above the tiles. On macOS, a
  click on the window below brings that window forward over the drawer, and
  keeping another app's window above it needs a window level change that only
  works with SIP disabled. SIP stays on, so **the drawer hides when it loses
  focus** (quake style).
- **Model**: a `drawer` module that takes the window out of the grid layout as
  the floating set does, kept in `~/.rift-grid/cells.json` with the cells.
- **Closing**: the window is parked off screen, as rift parks the windows of
  cells not shown, so the app stays alive.
- **Opening**: set the frame to the edge's rectangle (a share of the screen, a
  smallest size, a gap) with AX, then raise and focus. A slide can reuse
  `ui/grid_motion.rs`'s capture overlay.
- **Triggers**: keys first; the menu bar's menu in place of subline's dock
  list. The pointer pushed against a screen edge (the event tap) could come
  later. Trackpad edge swipes are not worth it.
- **What it opens**: the browser through `URLForApplicationToOpenURL` with an
  https URL, Finder with `open`. macOS has no default terminal, so the
  terminal is a configured bundle id. "An open, empty drawer takes the next new
  window, for up to 15 s" maps onto rift's window-created events.
- **Smaller points**: the borders treat a drawer as a floating window.
  Terminals with their own hotkey window (iTerm) already cover the terminal
  case.

Size: M with keys and the menu, L with an edge trigger and a slide.

## Grid presets

subline's `docs/design/grid.md` ("격자 프리셋"). A grid's layout is saved as
TOML: each window's app id, title, working directory and an editable command.
Loading it starts the apps again and places the windows as they arrive. The
grid button's right-click menu saves a preset or loads one of the latest five.

On macOS:

- Upstream's `save_layout`/`restore_layout` only arrange windows that are
  open. `cells.json` matches windows by pid and window number, so nothing
  matches after a reboot.
- Starting apps through NSWorkspace, and placing the windows that arrive by
  reusing the `cells.json` matcher, is the core of it. The right-click menu on
  the Overview's grid buttons (`ui/mission_control/grid_bar.rs`) is easy.
- Risks: telling apart the windows of an app with many windows, and terminal
  working directories.

Size: M to L.

## A slide that follows the fingers

subline's `layout_here` and `gesture.rs`. A three-finger swipe moves the
cells 1:1 with the fingers and goes on with a spring on release. rift-grid's
swipes are one step each (`actor/gesture/grid.rs`).

On macOS the neighbouring cells have to be captured before the swipe starts.
The Overview's previews show such captures can be had, but the capture delay
when a swipe starts is the risk.

Size: L.

## Carrying a tile over the screen's edge

subline's `tiles.rs` (`tiles_lift`, an 8 mm edge). Held at the screen's edge
while dragged, a tile moves the screen one cell that way. rift-grid's drags
(`actor/drag/dock.rs`) only drop within the cell on screen.

On macOS, waiting at the edge would run `move_tile` while the drag goes on.
The risk is that macOS holds the dragged window while rift moves the windows
under it.

Size: M.

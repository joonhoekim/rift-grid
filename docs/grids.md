# Grids of cells

Workspaces come in two levels: a **grid** is a context (one
project, one kind of work), and a **cell** of the grid is what a display shows.
Most work fits in one 3x3 grid; more grids keep separate contexts apart.

## Terms

- **Grid**: 3x3 cells, numbered 1 to 9. The grid size is fixed; a fixed 3x3 is
  the mental model, and a per-grid size can be opened up later if it is ever
  needed.
- **Cell**: one rift virtual workspace. The display shows one cell at a time.
- **Current cell**: each grid remembers the cell it was last on, the center cell
  until then.

## Model (first step: grids per display)

Rift's virtual workspaces are a list per display. With `grid_columns = 3`, each
display gets 9 grids x 9 cells = 81 workspaces; grid g (1-based) is workspaces
`9(g-1) .. 9(g-1)+8`, read row by row from the top left. Grids are per display:
each display has its own grid 1 to 9.

A later step could give each grid to one display instead (Alt+N goes to the
display that has grid N, grids move between displays from the overview), as a
table of which display owns which grid; with one display the two are the same.

## Moving

| Key (seed config) | Command | What it does |
| --- | --- | --- |
| Alt+1..9 | `switch_to_grid = N` | Show grid N at its current cell |
| Alt+Ctrl+1..9 | `move_window_to_grid = N` | Send the focused window to grid N's current cell; focus stays |
| Alt+IJKL / arrows | `grid_focus` | Tile beside, or the next cell of this grid past its edge |
| Alt+Shift+IJKL / arrows | `grid_move` | Swap with the tile beside, or carry the window into the next cell |
| Alt+Cmd+IJKL | `grid_cell` | Next cell of this grid |
| Alt+H | `grid_center` | Center cell of the grid on screen |
| Alt+Tab | `focus_recent_window` | The window focused before this one, whichever cell or grid it is in; again goes back |
| Alt+O | `show_mission_control_all` | Overview; again for the whole grid, again to close |

Cells never lead into another grid: moving past the grid's edge stops.

## App rules

`grid = N` (1-based) in an app rule sends the window to grid N's current cell on
its display. `workspace` keeps its meaning (a cell by its index in the 81, or by
name), and `grid` wins when both are given.

## Menu bar

`[settings.ui.menu_bar] grid_maps`:

- `"all"` (default): a 3x3 map per grid, side by side, each after its number,
  for the grids that have windows plus the grid on screen. The grid on screen, number and map, is drawn at full strength.
- `"active"`: one map, the grid on screen, after its number when more than one
  grid has windows.

Clicking the item opens rift's menu; its Workspace submenu lists grids 1 to 9
with their window counts and keys, the grid on screen checked, and picking one
shows it. Stepping through workspaces one by one is left out of that menu.

## Overview

The overview shows the grid on screen and follows it as it changes: focus
moves, windows that open or close, and grids that are switched to or filled. A row of grid buttons at the top lists
the grids that have windows plus the one on screen, which is highlighted.
Clicking a button, or pressing Alt+N while the overview is open, switches to that
grid and the overview stays open on it. The last button, "+", is the first grid
not in use yet. Dropping a window's tile on a button sends the window to that
grid's current cell, as Alt+Ctrl+N does; the overview stays where it is.

Stages: the overview opens on the active cell at `grid_zoom` with
its neighbours around it; opening it again shows the whole grid, and a third time
closes it. Esc steps back out a stage at a time: from the whole grid to the cell
stage, from there out of the overview. `grid_fit = true` makes the whole grid the
only stage.

When the active cell changes in the first stage, the camera moves as
`[settings.ui.mission_control] grid_follow` says: `"reveal"` (default) moves only as far as it takes to show the active cell whole, a gap
from the edges; `"center"` centers the active cell every time.

Each window's tile carries its app and title at its bottom left corner
(`grid_labels`, on by default).

`grid_style = "zoom"` shows the overview differently, on the display in use:
the screen draws back to the same stages, each cell its wallpaper with captures
of its windows, and comes in again to the cell chosen when it closes. Arrows and
Tab move the focus as in the cards, and the overview follows it, along with
windows that open or close and grids switched to. Pointing at a cell selects it;
Return, a click or the overview key in the last stage goes to it. It moves with
the slide's `[settings.ui.grid_motion]` duration and easing, and needs Screen
Recording access; without it, or outside a grid, the cards show instead.
`"cards"` (default) is the overview above.

## Motion

`[settings.ui.grid_motion]` shows where a move went. Both parts are on by default.

- `pan`: the screen slides to the cell moved to, diagonally when the move
  skips across, over `duration_ms` (240, up to 10000). A window carried along
  by `grid_move` stays in place while the cells slide past it. Moving to another
  grid fades to it instead. The slide is drawn from screen captures on an overlay
  while rift moves the windows under it, so it needs Screen Recording access;
  without it, or while the Overview is open, moves show at once.
  - `easing`: how the slide gathers speed and slows down. `"glide"` (default)
    starts quickly and settles gently, `"smooth"` is slow at both ends,
    `"snappy"` leaves at once and lands softly, `"overshoot"` runs a little
    past the cell and comes back, `"spring"` settles with a small bounce, and
    `"custom"` takes `curve`, a cubic Bézier curve written as in CSS's
    `cubic-bezier()`: `curve = "0.2, 0.8, 0.2, 1"`.
  - `zoom`: how far the screen draws back between cells, e.g. `0.15`; 0
    (default) keeps it flat. Zoomed, the screen draws back over
    `zoom_out_time` of the slide's time (0.25), slides across while drawn
    back, and comes in again over `zoom_in_time` (0.3). The slide across
    starts near the end of drawing back and ends halfway through coming in.
- `hud`: a small map of the grid, its number beside it, for `hud_ms` (700)
  after the last move; the lit cell glides from the cell left to the cell
  arrived at.

A move made while a slide is still playing carries the slide on from where it
has got to.

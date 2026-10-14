# Grids of cells

Workspaces come in two levels, as in subline: a **grid** is a context (one
project, one kind of work), and a **cell** of the grid is what a display shows.
Most work fits in one 3x3 grid; more grids keep separate contexts apart.

## Terms

- **Grid**: 3x3 cells, numbered 1 to 9. The grid size is fixed; a fixed 3x3 is
  the mental model, and a per-grid size can be opened up later if it is ever
  needed.
- **Cell**: one rift virtual workspace. The display shows one cell at a time.
- **Current cell**: each grid remembers the cell it was last on, the centre cell
  until then.

## Model (first step: grids per display)

Rift's virtual workspaces are a list per display. With `grid_columns = 3`, each
display gets 9 grids x 9 cells = 81 workspaces; grid g (1-based) is workspaces
`9(g-1) .. 9(g-1)+8`, read row by row from the top left. Grids are per display:
each display has its own grid 1 to 9.

subline goes further (each grid belongs to one display, Mod+N goes to the display
that has grid N, grids move between displays from the overview). That can be
layered on later as a table of which display owns which grid; with one display
the two are the same.

## Moving

| Key (seed config) | Command | What it does |
| --- | --- | --- |
| Alt+1..9 | `switch_to_grid = N` | Show grid N at its current cell |
| Alt+Shift+1..9 | `move_window_to_grid = N` | Send the focused window to grid N's current cell; focus stays |
| Alt+IJKL / arrows | `grid_focus` | Tile beside, or the next cell of this grid past its edge |
| Alt+Shift+IJKL / arrows | `grid_move` | Swap with the tile beside, or carry the window into the next cell |
| Alt+Cmd+IJKL | `grid_cell` | Next cell of this grid |
| Alt+Cmd+H | `switch_to_workspace = 4` | Centre cell of grid 1 |
| Alt+Tab | `switch_to_last_workspace` | The previous cell, whichever grid it is in |

Cells never lead into another grid: moving past the grid's edge stops.

## App rules

`grid = N` (1-based) in an app rule sends the window to grid N's current cell on
its display. `workspace` keeps its meaning (a cell by its index in the 81, or by
name), and `grid` wins when both are given.

## Menu bar

`[settings.ui.menu_bar] grid_maps`:

- `"all"` (default): a 3x3 map per grid, side by side, for the grids that have
  windows plus the grid on screen. The grid on screen is drawn at full strength.
- `"active"`: one map, the grid on screen, after its number when more than one
  grid has windows.

## Overview

A row of grid buttons (the grids that have windows, the one on screen
highlighted). Clicking one, or Alt+N while the overview is open, shows that grid
in the overview without leaving it.

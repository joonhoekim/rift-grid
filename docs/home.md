# Home

Status: design, not built yet. It is subline's home (its `docs/design/home.md`,
`crates/compositor/src/home/`) brought to macOS as far as macOS allows. Where
this differs from subline, the reason is given.

Home is where new work starts: a searchable grid of the installed apps, over
the current cell. It works like Spotlight: it opens on a key or a swipe, takes
the keyboard, and goes away when the keyboard goes elsewhere.

## Terms

| Term | Means |
| --- | --- |
| **Home** | The app grid with its search box, over the current cell |
| **Slot** | One app in the grid: its icon and its name under it |
| **Frequent row** | The first row of the grid: the apps opened most, lately |
| **Style** | How home covers the screen: `full` or `window` |

## Opening and closing

- **Command** `home`, as subline's action `home`: closes home if it is open
  (on any display); otherwise opens it on the **focused display** (where keys
  act, as grid swipes use it). With the Overview open, it closes the Overview
  and opens home once the Overview has closed, as subline's home called from
  the Overview. It is a reactor command (`ReactorCommand`, as `open_settings`
  and `focus_display` are), so keys, `rift-cli` and the settings app's
  Keyboard page all have it. The reactor runs it, since only the reactor
  knows the focused display: it takes the focused display first, and its
  Space. A display showing a fullscreen app has no Space rift manages, and
  then the reactor sends no display. The reactor always tells home (see
  *Code*): home closes if it is open; otherwise it opens only when a display
  came with the command. It works with or without the grid (`rows = 0`);
  only the swipe needs the grid, since the swipe table is the grid's.
- **Key**: the seed binds `Alt + Space` (subline's `Mod+Space`). macOS's
  Spotlight keeps `Cmd + Space`.
- **Swipe**: a new `GestureAction::Home` ("Home"). The default four-finger
  table binds `down = "home"`; down was left to macOS's App Exposé before, and
  rift now takes it.
- **Stepping back out of the Overview**: today the swipe opposite the one
  bound to `overview` steps back only while that way has no action
  (`actor/gesture/grid.rs`). With `down = "home"` that would end. So while the
  Overview is open, the way opposite `overview` steps back whatever it is
  bound to; its own action runs only while the Overview is closed.
  `docs/grids.md`'s swipe table says so.
- **Not on its own**: subline's empty cell *is* home. On macOS an empty cell
  shows the desktop, and opening home there by itself would take the keyboard
  away from whatever app is in front. So home opens only on the command (key,
  swipe, CLI).
- **Not over macOS's Mission Control**: while it is open, the command and the
  swipe do nothing.
- **Not over a fullscreen app**: rift does not manage windows in a fullscreen
  app's Space, so home does not open there: the command and the swipe do
  nothing (rift claims a swipe that has an action wherever it is, so the swipe
  is not given back to macOS). Stage Manager is not supported by rift; home
  does not look for it.

Home closes, without doing anything else, when:

- `Esc` is pressed with an empty search (with text in it, `Esc` clears the
  text first);
- the command runs again;
- home stops being the key window: a click outside it, `Cmd + Tab`, a click
  on another display, another app taking focus;
- the current cell or grid changes, by a key, a swipe, a click or an app
  activated in another cell (focusing another display without changing a
  cell takes the key window away, which closes it as above);
- the Overview opens;
- the active macOS Space changes, macOS's own Mission Control or Apps
  (Launchpad) opens, the screen locks or the Mac sleeps;
- the display it is on goes away or changes size.

A move that runs into the grid's end while home is open lights the edge over
home (the light is above it) and leaves home open: nothing moved.

Launching an app also closes it (see *Launching*). Only one home is open at a
time.

Closing forgets the search, the picked slot and the scroll: home always opens
at the top with an empty search, as subline's does.

When home closes without launching, the app that was in front gets the
keyboard back (see *The window*).

## Styles

`[settings.grid.home] style`, as subline's `home.style`:

- **`full`** (default): home covers the whole display: a blurred backdrop over
  the windows, the clock and the date at the top, the search box under them,
  the grid under that, to the bottom of the screen. A click anywhere that is
  not a slot or the search box closes it.
- **`window`**: a panel in the middle of the display's usable area (below the
  menu bar, above the Dock), `window_width` x `window_height` of it (0.6 x 0.7,
  each from 0.3 to 1.0); never narrower than three slots. The panel has the
  search box and the grid, no clock (no room, as subline). The windows around
  it stay in place and are darkened by 0.5. A click outside the panel closes
  home and goes no further (the window under it does not get it); a click on
  an empty part of the panel does not close it.

Both styles are one window that covers the display; only what it draws
differs. That keeps one path for keys, clicks and closing.

## The window

Home is an `NSPanel` in rift's own process (rift runs `NSApp` with the
accessory policy, no Dock icon). It is the first AppKit window rift itself
shows, so it has some conditions:

- a `define_class!` subclass of `NSPanel` that returns `true` from
  `canBecomeKeyWindow`;
- style `borderless | nonactivatingPanel`, given when it is made (changing
  the mask later is unreliable). It becomes the key window without
  activating rift, so the app in front stays active. This is how Spotlight-like
  launchers (Raycast, Alfred) work;
- when home closes without launching, macOS normally gives the keyboard back
  to the front app's window. As a fallback home remembers
  `frontmostApplication` when it opens and activates it again if no window of
  it became key, but only when home closed itself (`Esc`, the command again,
  a click on home's empty part). A close because something else took the
  keyboard (another app, `Cmd + Tab`, a cell change, the Overview, a Space
  change, the lock) leaves the keyboard where that put it;
- window level 25 (`NSStatusWindowLevel`): above the windows and the menu bar,
  below the pop-up menu level, so its own right-click menu and the input
  method's candidate window draw above it. The Overview (a WindowServer
  overlay at level 19) never shows together with home;
- joins all Spaces, so it can open on whichever Space is active;
- made when rift starts (hidden), not on the first opening, with `defer: NO`
  and not one-shot, so it has its window number from the start;
  `hidesOnDeactivate` is set to `NO` (an `NSPanel` hides when its app
  deactivates by default, and rift is never the active app);
- the pointer must stop at it. Rift's "window under the pointer"
  (`window_server::get_window_at_point`) looks through rift's own windows
  unless they are document or modal windows, so as it is, focus-follows-mouse
  would focus the window under home, a click on a slot would also focus that
  window (the mouse-up check focuses what is under the cursor), and a
  modifier drag on home would drag it. So `window_server` keeps home's window
  number in a `static AtomicU32` (0 while there is none; the reactor and the
  input thread read it, the main thread sets it, as `OVERVIEW_OPEN` is
  shared), and a point over it resolves to home, both for the window under
  the pointer and for "is this point covered by another window": nothing
  under it is focused or dragged (tested). The borders already keep only
  managed windows and are below level 25.

The search box is a real `NSTextField`, the first responder while home is
open. This is why home is an AppKit window and not a WindowServer overlay
(`CgsWindow`) as the Hud and the Overview are: an overlay cannot be the key
window, and keys read from rift's event tap (as the Overview does) cannot go
through an input method, so Korean app names could not be typed.

- `full` draws an `NSVisualEffectView` (blur) over the whole display;
  `window` draws a dark layer over the display with an `NSVisualEffectView`
  panel with rounded corners in it.
- The grid is an `NSScrollView` with a layer-backed document view of slot
  views (each draws its icon and name in `updateLayer`), so scrolling and
  momentum are macOS's own (wheel and trackpad). As subline's wheel and
  two-finger scroll, it does not go past the ends (`verticalScrollElasticity`
  none); subline's resistance is for a finger dragging the grid, which a Mac
  does not have.

The window is made once and kept: opening shows it, closing hides it. Each
time it opens it is set to the display:

- the display is the `NSScreen` whose `NSScreenNumber` is the display id the
  reactor sent; its `frame` (the whole display) is the window's frame, and
  its `visibleFrame` (without the menu bar and the Dock, wherever the Dock
  is) is where the `window` panel is centred. AppKit's frames are
  bottom-left based; the reactor's are top-left based, so home uses the
  `NSScreen`'s own frames and not the reactor's;
- with the menu bar or the Dock hidden automatically, `visibleFrame` is
  nearly the whole display, and the panel uses it as it is;
- on a display with a notch, the `full` clock goes below `safeAreaInsets.top`;
- the slot views follow the display's scale (`viewDidChangeBackingProperties`
  sets their layers' `contentsScale`), so icons are sharp on every display.

**Motion**: opening fades in and rises 12 points into place (subline's
"comes up into place"), closing fades out; both short (about 0.15 s and 0.1 s) and
scaled by `[settings.grid] motion` as the other fork motions are.

## The apps

The apps are the `.app` bundles in:

- `/Applications` (`Utilities`, `Nix Apps` from nix-darwin, and the like are
  folders in it);
- `/System/Applications`;
- `/System/Library/CoreServices/Applications` (Keychain Access, Archive
  Utility, Directory Utility, …);
- `~/Applications` (`Home Manager Apps`);
- Finder (`/System/Library/CoreServices/Finder.app`), which is not in any of
  those.

Folders are read four levels down, as upstream's scanner does. Links are
followed, both to bundles and to folders: Safari in `/Applications` is a link
into `/System/Cryptexes/App`, Nix's apps are links into the store, and
`~/Applications/Home Manager Apps` is a link to a store folder. A folder
already read (by its real path) is not read again, so a link loop ends. A
bundle inside another bundle is not looked at. An app appears once, by bundle
id; the first folder in the list above wins. Left out:
`LSBackgroundOnly` bundles and bundles whose name starts with a dot
(`.Karabiner-VirtualHIDDevice-Manager.app`). Menu bar apps (`LSUIElement`)
stay: they have no Dock icon, but people start them from a launcher.

For each app home keeps: the bundle id, the path, the **name** (Finder's name,
`NSFileManager displayNameAtPath`: localized, "계산기"), and **other names**
to search by: the bundle's `CFBundleName` and `CFBundleDisplayName` from its
`Info.plist` and the file name, so `calc` finds 계산기 too. The icon is
`NSWorkspace iconForFile`, loaded when its slot is first drawn and cached.

Upstream already has a scanner for the settings app's app pickers
(`sys/installed_apps.rs`: `/Applications`, `/System/Applications` and
`~/Applications`, four levels, name and bundle id). Links to bundles work
there, but not links to folders (`file_type().is_dir()`), so
`~/Applications/Home Manager Apps` is never read. Home does not change it: the
settings app's other pickers keep it, and the fix is only proposed upstream
(`docs/upstream.md`). Home's fuller list is its own module
(`ui/home/apps.rs`). The settings app's Home page picks hidden apps from that
list, not from `installed()` (read on the settings app's job thread, as
`installed()` is), so the picker and home show the same apps.

**Keeping up**: each time home opens, it compares the modification dates of
every folder it read, at every level, with the ones it read, without
following links (`lstat`: a store folder's date is 1970; the folder holding
the link changes when Home Manager or nix-darwin replaces it, and
`/Applications/Nix Apps` changes when a link in it does while `/Applications`
does not). If any changed, it reads the apps again. That is a few dozen `lstat`s
per opening, and an app installed or removed shows the next time home opens.
(subline watches the folders; on macOS reading again on opening gives the
same result with less to keep running.)

The list is first read when rift starts, on a background thread, so the first
opening does not wait. Reading again happens on the same thread; home shows
the old list until the new one is ready. An app removed while home is open
stays until the next opening; opening it then fails with a notice.

## The grid

- Slots in rows, as many columns as fit the width (subline's slot is 108 x
  116 logical px with a 60 px icon; the same in points here). Names are cut to
  fit on two lines with an ellipsis.
- Order: by name, as Finder sorts (`localizedStandardCompare`).
- **Hidden apps** (`hidden = ["com.apple.Chess", …]`, bundle ids, case
  ignored) are left out of the grid, the search and the frequent row.
- **Frequent row** (`frequent = true`): with no search, the first row is the
  apps opened most lately, as many as fit in one row, then a thin line, then
  all the apps as usual (the frequent ones are there again). If nothing was
  ever opened, there is no row and no line.
- **What counts as opened**: each opening from home, whether the app was
  running or not. subline also counts the apps its dock and its other
  launchers open; here the Dock and Spotlight are macOS's and rift does not
  see them open an app. Counting every process start instead would fill the
  row with login items and helpers, and would miss apps that stay running.
  Switching to an app (`Cmd + Tab`, a click) is not counted either. Each time
  is written under the bundle id in `~/.rift-grid/launches.json`
  (`data_dir()`), the last 20 per app. The score is each time halved per 7
  days of age, summed; ties by name.

## Search

- The search box has the keyboard as soon as home opens: typing searches. The
  input method works as in any text field: Korean composes in the box, and
  the grid follows the text as it is being composed, composing part included.
  Whether the text field tells its delegate (`controlTextDidChange:`) while
  text is being composed is checked first in the key-window prototype; if it
  does not, the panel gives the box its own field editor
  (`windowWillReturnFieldEditor:toObject:`) whose `setMarkedText:…` and
  `insertText:…` report the text.
- It matches each word of the search, case and accents ignored, against the
  name and the other names. Order, as subline's: names that start with the
  search, then names that contain it, then apps found only by their other
  names; within each, by name.
- With text in the box, the frequent row and its line are not shown.
- No results: "No apps found".

**Keys**, in the text field:

| Key | Does |
| --- | --- |
| typing | searches; the first result is picked |
| `↑` `↓` | the slot in the row above or below (the same place, or the row's last); nothing on the first or last row |
| `←` `→` | the slot before or after, across rows (as subline; the text cursor does not move with them) |
| `Return` | opens the picked app; with nothing picked (no search, or no results), does nothing |
| `Esc` | clears the search; with no search, closes home |
| `Cmd + A` `C` `X` `V` `Z` | the text field's editing |

- While the input method is composing (`hasMarkedText`), `Return`, the arrows
  and `Esc` are the input method's, as in any text field; home takes them (in
  `control:textView:doCommandBySelector:`) only once nothing is being
  composed.
- The pointer does not pick: hovering a slot only lightens it, and a click
  opens it.
- With no search, an arrow key starts picking from the first slot shown (the
  first frequent app when the row is there).
- The picked slot has a rounded plate behind it (the accent color, darkened),
  and the grid scrolls only as far as the picked row needs to be seen.
  Changing the search picks the first result again. Arrow keys repeat when
  held.
- A text field's `Cmd + C`/`V`/`A`/`Z` come from the main menu's Edit menu,
  and rift's own process sets no main menu (only the settings app does). So
  the panel's `performKeyEquivalent:` sends `selectAll:`, `copy:`, `cut:`,
  `paste:`, `undo:` and `redo:` to the field editor itself; nothing outside
  the panel changes.

Rift's own keys still work while home is open (they come from the event tap,
before the text field): `Alt + Space` closes home, and a key bound to a rift
command runs it (and closes home if it moves the view). Plain typing is not a
rift binding and reaches the text field.

## Launching

A click on a slot, or `Return`, opens the app with `NSWorkspace
openApplicationAtURL` (the configuration activates it) and closes home.

- An app that is not running starts, and its first window is placed as rift
  places any new window: in the current cell of the display it opens on (in
  the global layout mode, the display under the pointer).
- An app that is running is activated: macOS brings its windows forward and
  rift goes to the cell of its focused window, as when it is picked in the
  Dock. macOS 14 and later may ignore an activation asked for by an app that
  is not active, and rift never is while home is open. So when the open's
  completion handler has run (off the main thread) and the app is still not
  active (`NSRunningApplication isActive`), home asks the reactor to raise
  that app (by pid); the reactor knows its windows and raises the focused one
  as it raises any window (`window_server::make_key_window` needs the window
  id). This is checked by hand with the key-window prototype.
- A running app with no windows gets macOS's reopen event, so most apps open
  a new window. This is subline's `launch.raise` (on by default there)
  without anything to build; there is no setting to turn it off.
- If opening fails, a notice says so (as `exec` does), and home stays closed.

subline puts a placeholder tile where the window will go, before the window
comes. That needs a tile without a window in rift's layout, and is left for
later (*Later*).

## The app menu

A right-click (or `Ctrl` click) on a slot opens a menu (`NSMenu`) for it:

- **Open**
- **Show in Finder**
- **Hide from Home**: adds the bundle id to `hidden` in the config file, so
  the file stays the one source; undo is in the settings app, as subline. The
  config actor gets a new event that edits the current source in place
  (through its `edit_source`, as `ApplySource` does, but without a revision
  to match and without an answer to wait for), so the main thread does not
  wait and a save from the settings app in between is not lost; a save the
  config actor refuses (the file changed on disk in a way it cannot merge)
  shows a notice;
- **Quit** (only while it is running)

subline's menu also has the app's desktop actions ("New window") and pinning
to the dock. macOS apps have no such list, and the Dock's own menu pins.

## Settings

`[settings.grid.home]`, as subline's `[home]`:

| Key | Default | Means |
| --- | --- | --- |
| `style` | `"full"` | `full` or `window` |
| `window_width` | `0.6` | the panel's width, a share of the usable area (0.3–1.0), `window` only |
| `window_height` | `0.7` | the panel's height, the same |
| `frequent` | `true` | the frequent row |
| `hidden` | `[]` | bundle ids left out of home |

A reload while home is open applies `hidden` and `frequent` to what is shown;
a change of `style` or the panel's size closes home, and the next opening
uses it.

subline's `scrim` and `label_shadow` keep white names readable over a bright
background picture; here the blur and the darkened layer do that, so they are
not brought over.

The settings app gets a **Home** page, in the Workspace section after
Overview, as subline's: the frequent row, the style with the panel's size
(enabled when the style is `window`), and **Hidden apps**: a list of the
hidden apps with a remove button each and an add button that picks from the
installed apps (`ui/home/apps.rs`'s list, read in the settings process, off
the main thread). Search finds the page and each setting ("Home › …").

## Code

As the fork's other parts, in their own modules with small hooks in upstream's
files:

| Where | What |
| --- | --- |
| `src/common/config/grid_home.rs` | `HomeSettings`, under `GridSettings.home` |
| `src/common/config/grid_gestures.rs` | `GestureAction::Home` |
| `src/actor/home.rs` | the home actor on the main thread (as the Overview's `actor/mission_control.rs`): `Event::{Toggle(Option<display>), Close, Configure, …}`, the open state, the display it is on. It also watches, on its own (`NSWorkspace` and distributed notifications, no hook), the active Space changing, the screens changing, sleep and the screen locking; each closes home. Apps (Launchpad) takes the key window, which closes home as any app does (checked by hand) |
| `src/ui/home/` | `apps.rs` (reading the apps), `search.rs` (matching and order), `launches.rs` (the opened-apps file and its score), `grid.rs` (slots, layout, picking with the arrow keys, drawing), `panel.rs` (the `NSPanel` subclass, its views, its keys) |
| `crates/rift-protocol/src/commands.rs` | `ReactorCommand::Home`, `#[command(group = 2, label = "Home")]`, with the grid's commands |
| `src/bin/rift-cli.rs` | `rift-cli home` (its subcommands are written by hand) |
| `src/actor/reactor/grid_home.rs` (fork) | runs `home` (matched where the reactor matches its other `ReactorCommand`s): the focused display is the menu bar's (`sys::screen::active_menu_bar_display_uuid()`, asked when the command runs), found in `space_state.screens`; no display when that screen's `space` is `None` (a fullscreen app). Nothing at all while macOS's Mission Control is active. Not `command_context_space()` or `active_display_space()`: both fall back to another display's Space when the focused one has none, which would open home on the wrong display; sends `WmEvent::Home { display: Option<…> }` over `communication_manager.wm_sender`. Raises an app's focused window on `Event::RaiseApp(pid)` from home. Compares each display's active workspace before and after every event and sends `WmEvent::CloseHome` when one changed (the snapshot is taken at the top of `handle_event_inner`, before the grid's early returns such as `MoveGridToDisplay`, and compared on those returns too; a nested `handle_event_inner` may send it twice, which is harmless): this covers keys, swipes, clicks, grid switches from the Overview and an app activated in another cell, with the cell motions on or off. A window opening or closing in the current cell does not send it |
| `src/actor/reactor.rs` | hooks: the `home` arm and `Event::RaiseApp`; in `handle_event_inner`, the snapshot at the top and the comparison on each return, before `grid_motion_after` so home is told before a slide's capture starts; `WmEvent::CloseHome` when macOS's own Mission Control becomes active (`MissionControlState`) |
| `src/actor/wm_controller.rs` | hook: `WmEvent::Home` sends `Toggle` to home, or, with the Overview open, has the Overview close and open home after (below); `WmEvent::CloseHome` sends `Close`; `ConfigUpdated` sends `Configure` to home, next to the Overview's |
| `src/actor/mission_control.rs` | hooks: `MissionControlActor::open` closes home; `Event::DismissThenHome(display)` closes the Overview as `Dismiss` does and, in `close()` after `SetMissionControlActive(false)` (so the input tap no longer turns keys into Overview input), sends home its `Toggle`; with no Overview session (already closed), it sends the `Toggle` at once. The wm controller, the Overview and home share the main executor, so a `Dismiss` followed by a `Toggle` would not keep this order |
| `src/actor/config.rs` | hook: an event that edits the current source in place, for "Hide from Home" |
| `src/sys/window_server.rs` | hook: home's window number (`AtomicU32`), which the pointer queries stop at |
| `src/actor/reactor/grid_motion.rs` (fork) | `begin_pan` puts home's window number (read from the atomic) in the capture's `leave_out`; if the cached shareable content does not know it, it is fetched again first (`fetch_now`), since the capture drops windows it does not know from the list silently and `expire()` only refetches next time. Home is made at startup and the list holds off-screen windows, so this happens at most once |
| `src/bin/rift.rs` | hook: make home's channel before `WmController::new` (as the Overview's is); give home's sender to the wm controller and the Overview actor (both constructors take one more sender), and give home the reactor's sender and the config actor's; read the apps on a background thread |
| `src/actor/reactor/grid_gestures.rs` (fork) | the swipe: `grid_swipe` works out the swipe's action first (it needs no Space); for `Home` it returns while macOS's Mission Control is active (`is_mission_control_active()`, the check `gesture_space_active` makes for the others) and otherwise runs the `home` command, before `command_context_space()`, so the display comes from the menu bar's display as above and a fullscreen display does nothing. The grid being on needs no check there: the gesture actor makes no grid swipes without it |
| `src/actor/gesture/grid.rs` (fork) | the Overview's way back, whatever that way is bound to |
| `src/ui/settings/` | `Category::Home` in `CATEGORIES` (`mod.rs`), its place in `grid_sidebar::ORDER`, `grid_pages.rs`, search entries |

`docs/upstream.md`'s hooks table gets the new hooks, and its proposals the
`installed_apps` folder link fix.

**Tests** (no live window, nothing that takes the keyboard while the owner
works):

- reading the apps from a folder of made-up bundles (`Info.plist` only):
  bundle ids, first folder wins, `LSBackgroundOnly` and dot names left out,
  `LSUIElement` kept, links to bundles and to folders followed;
- search order on a fixed list, including Korean names and other names;
- the frequent score and the file (times capped at 20, ages halved, hidden
  apps left out);
- picking with arrow keys on rows of different lengths, starting on the
  frequent row;
- columns for a width, and the panel's size and its three-slot floor;
- the swipe table: with the Overview open, the way opposite `overview` steps
  back even when it is bound to `home`;
- a point over home's window resolves to home, not to the window under it;
- `CloseHome` is sent on a cell change by a key and by an app activated in
  another cell, with the cell motions on and off, before the slide starts;
  not on a window opening in the current cell;
- `home` on a display showing a fullscreen app sends no display, and closes
  an open home;
- the config: defaults, `deny_unknown_fields`, the seed;
- a render of the panel's views to a PNG, both styles, through the layer tree
  (`renderInContext`: `cacheDisplayInRect`, which the settings render uses,
  draws neither the blur nor layers), the blur drawn flat. Like
  `settings_native`, it is an `--ignored` test that needs a logged-in session;
  it draws into an offscreen context and never shows a window.

The key-window behaviour (becoming key without activating, the keyboard going
back on close, composing Korean, a running app coming forward when opened) is
checked by hand once, early, with a small panel before the rest is built.

## Later

The second step, in this order:

1. **Command palette**: a search starting with `>` lists rift's commands
   (`CommandCatalog`, each with its keys as on the Keyboard page), then the
   user's own commands (`commands = [{ name, run }]`), then "Run" for the typed
   text.
2. **Calculator**: an expression gives its answer as the first row; `Return`
   copies it.
3. **Open windows**: up to five windows whose title or app matches, most
   recent first; picking one goes to it.
4. **Settings**: up to four of the settings app's search results; picking one
   opens the settings app on that page.
5. **Categories**: `[[settings.grid.home.categories]]`, boxes on the grid as
   subline's; "Fill from the apps' categories" reads `LSApplicationCategoryType`.
6. **Placeholder tile** while an app starts.

# Installing rift-grid

rift-grid is built from source; there are no prebuilt binaries yet. The steps
are the same with or without nix-darwin up to the point where something has to
start it at login: without nix-darwin that is `rift-grid service`, with it a
`launchd.user.agents` entry.

rift-grid uses names of its own wherever upstream rift has one, so the two can
be installed side by side:

| | rift-grid | upstream rift |
| --- | --- | --- |
| Binaries | `rift-grid`, `rift-grid-cli` | `rift`, `rift-cli` |
| Config | `~/.config/rift-grid/config.toml` | `~/.config/rift/config.toml` |
| Saved layout | `~/.rift-grid/layout.ron` | `~/.rift/layout.ron` |
| Mach service, launchd label, bundle id | `io.github.joonhoekim.rift-grid` | `git.acsandmann.rift` |
| Logs (`service install`) | `/tmp/rift-grid_$USER.*.log` | `/tmp/rift_$USER.*.log` |

## Before you start

- macOS 26 or 27 (the versions it is run on), Apple silicon or Intel.
- Rust stable, through [rustup](https://rustup.rs), and the Xcode command line
  tools (`xcode-select --install`).
- System Settings > Desktop & Dock > Mission Control > **Displays have separate
  Spaces** turned on. rift-grid exits at once when it is off. Changing it takes a
  logout.
- No other tiling window manager running, upstream rift included. Upstream
  rift can stay installed, but stop it and keep it from starting at login
  (`rift service stop`, then `rift service uninstall`).

## Build and install

```sh
git clone --branch grid https://github.com/joonhoekim/rift-grid.git
cd rift-grid
cargo build --release --bin rift-grid --bin rift-grid-cli

mkdir -p ~/.local/bin
for name in rift-grid rift-grid-cli; do
  install -m 755 target/release/$name ~/.local/bin/.$name.new
  mv -f ~/.local/bin/.$name.new ~/.local/bin/$name
done
```

The binaries are moved into place rather than copied over: overwriting the
executable of a running rift-grid makes macOS kill it.

`~/.local/bin` has to be on your `PATH`: key bindings that run `rift-grid-cli`
find it there, and `rift-grid service install` writes the first `rift-grid` it
finds on `PATH` into the launch agent.

`rift-grid --version` prints the version and the commit it was built from, for
example `rift-grid 0.7.0+afa9692da8ef`.

### Signing, so permissions survive rebuilds (recommended)

rift-grid needs Accessibility and Screen Recording. macOS ties those grants to the
binary's code signature, and a plain `cargo build` gets an ad-hoc signature
that changes with every build. After a rebuild the switches in System Settings
still look on, but rift-grid is refused, and macOS does not ask again.

Signing with a certificate of your own keeps the grants across rebuilds:

1. Keychain Access > Certificate Assistant > Create a Certificate. Name it, for
   example, `local codesign`; Identity Type: Self Signed Root; Certificate
   Type: Code Signing.
2. After each install, sign both binaries with it, before the `mv` above:

   ```sh
   codesign --force --sign "local codesign" \
     --identifier io.github.joonhoekim.$name \
     --timestamp=none ~/.local/bin/.$name.new
   ```

`codesign -d -r- ~/.local/bin/rift-grid` should then show a requirement naming the
certificate (`certificate leaf = H"…"`), not `cdhash`. The certificate is only
good on your own machine; it does not make the binary something you can hand
to others.

## Configuration

rift-grid reads `~/.config/rift-grid/config.toml`. Without that file it runs on
the built-in defaults, which are upstream's: no grid, and the keys of upstream
rift. A config of upstream rift's can be copied over and works as it is. To turn the grid on, start from the default file:

```sh
mkdir -p ~/.config/rift-grid
cp rift.default.toml ~/.config/rift-grid/config.toml
```

then, in that file:

- under `[virtual_workspaces]`, add `grid_columns = 3`;
- under `[settings.ui.mission_control]`, set `enabled = true` for the grid
  overview;
- in `[keys]`, replace the bindings that use the same keys with the grid ones
  (the default `Alt + H/J/K/L`, `Alt + 1..3`, `Alt + Tab`):

```toml
"Alt + I" = { grid_focus = "up" }
"Alt + K" = { grid_focus = "down" }
"Alt + J" = { grid_focus = "left" }
"Alt + L" = { grid_focus = "right" }
"comb1 + I" = { grid_move = "up" }
"comb1 + K" = { grid_move = "down" }
"comb1 + J" = { grid_move = "left" }
"comb1 + L" = { grid_move = "right" }
"Alt + Cmd + I" = { grid_cell = "up" }
"Alt + Cmd + K" = { grid_cell = "down" }
"Alt + Cmd + J" = { grid_cell = "left" }
"Alt + Cmd + L" = { grid_cell = "right" }
"Alt + H" = "grid_center"
"Alt + Tab" = "focus_recent_window"
"Alt + O" = "show_mission_control_all"
"Alt + 1" = { switch_to_grid = 1 }
"Alt + 2" = { switch_to_grid = 2 }
"Alt + 3" = { switch_to_grid = 3 }
"Alt + Ctrl + 1" = { move_window_to_grid = 1 }
"Alt + Ctrl + 2" = { move_window_to_grid = 2 }
"Alt + Ctrl + 3" = { move_window_to_grid = 3 }
```

`comb1` is the default file's name for Alt + Shift. Up to nine grids can be
bound the same way. [grids.md](grids.md) explains the model and the rest of the
commands; [FORK.md](../FORK.md) lists the settings the fork adds. The config is
reloaded when the file changes.

Upstream's documentation and default config call the client `rift-cli`; with
rift-grid it is `rift-grid-cli`, with the same subcommands.

## Running it

### Without nix-darwin

```sh
rift-grid service install    # writes ~/Library/LaunchAgents/io.github.joonhoekim.rift-grid.plist
rift-grid service start
```

The agent starts rift-grid at login and restarts it after a crash. The other
commands:

| Command | What it does |
| --- | --- |
| `rift-grid service restart` | Restart it, for example after installing a new build |
| `rift-grid service stop` | Stop it until the next `start` or login |
| `rift-grid service uninstall` | Remove the launch agent (stop it first) |

The plist holds the `PATH` of the shell that ran `install`. If you change where
the binaries are, run `rift-grid service stop`, `uninstall` and `install` again.

Logs go to `/tmp/rift-grid_$USER.out.log` and `/tmp/rift-grid_$USER.err.log`.

To try it once without an agent, run `~/.local/bin/rift-grid` in a terminal; it
stays in the foreground and logs there.

### With nix-darwin

Let nix-darwin own the launch agent instead of `rift-grid service`, and build the
binary outside nix as above. A binary built by nix is ad-hoc signed and has a
new store path on every change, so the permissions would have to be granted
again after each rebuild.

```nix
{ config, ... }:
let user = "you"; in
{
  system.defaults.spaces.spans-displays = false; # "Displays have separate Spaces" on

  launchd.user.agents.rift-grid = {
    command = "/Users/${user}/.local/bin/rift-grid";
    # Key bindings that run rift-grid-cli need it; an agent gets no login-shell PATH.
    path = [ "/Users/${user}/.local/bin" config.environment.systemPath ];
    serviceConfig = {
      RunAtLoad = true;
      # Running while the binary exists. Plain KeepAlive would turn a missing
      # binary into a restart loop; with PathState launchd waits for it.
      KeepAlive.PathState."/Users/${user}/.local/bin/rift-grid" = true;
      StandardOutPath = "/tmp/rift-grid.log";
      StandardErrorPath = "/tmp/rift-grid.log";
    };
  };
}
```

If upstream rift also has a launch agent (its own `rift service install`, or an
agent in your nix-darwin config), remove it. Do not also run
`rift-grid service install`: two agents would start two copies.

nix-darwin names the agent `org.nixos.rift-grid`:

```sh
launchctl kickstart -k gui/$(id -u)/org.nixos.rift-grid   # restart
launchctl print gui/$(id -u)/org.nixos.rift-grid          # state, last exit code
```

The config file can be managed by nix too (home-manager's `xdg.configFile`),
but a store symlink is read-only, which leaves the settings window and the
menu bar unable to save changes. Copying a seed file on activation when none
exists keeps it writable.

## Permissions

On first start rift-grid asks for:

- **Accessibility** (System Settings > Privacy & Security > Accessibility):
  required; it does nothing without it.
- **Screen & System Audio Recording**: for the live previews in the overview
  and the slide between cells. Without it those are off and it says so in its
  log.

The entry is for the binary, `~/.local/bin/rift-grid`. Restart it after
granting either one.

## Updating

```sh
cd rift-grid
git pull
cargo build --release --bin rift-grid --bin rift-grid-cli
# install (and sign) as above, then:
rift-grid service restart   # or: launchctl kickstart -k gui/$(id -u)/org.nixos.rift-grid
```

Unsigned builds need both permissions granted again after each update: remove
the old `rift-grid` entry in each list with the minus button, add the new binary,
and restart it.

## Uninstalling

Stop rift-grid and remove what starts it:

```sh
# without nix-darwin
rift-grid service stop
rift-grid service uninstall

# with nix-darwin: remove the launchd.user.agents.rift-grid entry and rebuild
```

Then remove the files:

```sh
rm -f ~/.local/bin/rift-grid ~/.local/bin/rift-grid-cli
rm -rf ~/.config/rift-grid ~/.rift-grid   # config, saved layouts, master file
rm -f /tmp/rift-grid*.log
```

Finally, remove the `rift-grid` entries under Privacy & Security > Accessibility and
Screen & System Audio Recording (select, minus button). The signing
certificate, if you made one, is in Keychain Access under "My Certificates".

## When it does not start

- **Exits right away**: run `~/.local/bin/rift-grid` in a terminal to see why. The
  usual reason is "Displays have separate Spaces" being off.
- **Runs but does nothing**: Accessibility is missing, or was granted to an
  earlier build (see Signing).
- **Overview previews are blank**: Screen Recording is missing or was granted
  to an earlier build; grant it and restart rift-grid.
- **Upstream keys still apply, no grid**: rift-grid found no file at
  `~/.config/rift-grid/config.toml` and runs on the defaults.
- **Exits with a `called Result::unwrap()` panic naming a field**: the config
  file does not parse; the message names what is missing or wrong.

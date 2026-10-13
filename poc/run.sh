#!/bin/sh
# Run this fork in the foreground in place of the installed rift, with the grid
# config. The installed rift (launch agent org.nixos.rift) comes back on exit.
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
domain="gui/$(id -u)"
plist="$HOME/Library/LaunchAgents/org.nixos.rift.plist"

restore() {
    trap - EXIT INT TERM
    launchctl bootstrap "$domain" "$plist" 2>/dev/null
    echo "restored org.nixos.rift"
}

launchctl bootout "$domain/org.nixos.rift" 2>/dev/null
trap restore EXIT INT TERM
"$root/target/release/rift" --config "$root/poc/config.toml" "$@"

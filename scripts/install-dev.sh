#!/bin/sh -e
#
# Build this checkout, install it over the running rift-grid, and restart it.
# The development loop: edit, run this, try it. Nothing outside the repo is needed
# beyond a signing identity already in the keychain.
#
#   scripts/install-dev.sh
#
# Signed with the machine's own certificate so the Accessibility and Screen
# Recording grants survive the rebuild (docs/install.md, "Signing").
#   MAC_SIGN_ID   certificate name (default "<user> local codesign")
#   RIFT_GRID_BIN install directory (default ~/.local/bin)

cd "$(dirname "$0")/.."
CN="${MAC_SIGN_ID:-$(id -un) local codesign}"
BIN="${RIFT_GRID_BIN:-$HOME/.local/bin}"

if ! security find-certificate -c "$CN" >/dev/null 2>&1; then
  echo "No signing certificate named \"$CN\" in the keychain (set MAC_SIGN_ID)." >&2
  exit 1
fi

cargo build --release --bin rift-grid --bin rift-grid-cli

mkdir -p "$BIN"
for name in rift-grid rift-grid-cli; do
  # Moved into place, never overwritten: rewriting the executable of a running
  # rift-grid makes macOS kill it.
  install -m 755 "target/release/$name" "$BIN/.$name.new"
  codesign --force --sign "$CN" --identifier "io.github.joonhoekim.$name" \
    --timestamp=none "$BIN/.$name.new"
  mv -f "$BIN/.$name.new" "$BIN/$name"
done
# A build-from-source installer may stamp the commit it last installed to skip
# rebuilds; this build is not that commit, so drop the stamp.
rm -f "$BIN/.rift-grid.commit"
echo "installed $("$BIN/rift-grid" --version | head -1)"

# Restart whichever launch agent runs it: nix-darwin's, or `rift-grid service install`'s.
uid="$(id -u)"
for label in org.nixos.rift-grid io.github.joonhoekim.rift-grid; do
  if launchctl print "gui/$uid/$label" >/dev/null 2>&1; then
    launchctl kickstart -k "gui/$uid/$label"
    echo "restarted $label"
    exit 0
  fi
done
echo "no rift-grid launch agent is loaded; start it yourself"

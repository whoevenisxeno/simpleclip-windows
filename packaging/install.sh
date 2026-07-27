#!/usr/bin/env bash
# Build SimpleClip and install it into the user's local paths.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${XDG_BIN_HOME:-$HOME/.local/bin}"
APPS="$HOME/.local/share/applications"

echo "==> Building release binaries"
cargo build --release --manifest-path "$ROOT/Cargo.toml"

mkdir -p "$BIN" "$APPS"
for b in scd sc sc-gui; do
    install -m 0755 "$ROOT/target/release/$b" "$BIN/$b"
    echo "    installed $BIN/$b"
done

sed "s|@BIN@|$BIN|g" "$ROOT/packaging/simpleclip.desktop" > "$APPS/simpleclip.desktop"
echo "    installed $APPS/simpleclip.desktop"

cat <<EOF

==> Done.

Start the daemon on login and bind a save hotkey. On Hyprland, add to
~/.config/hypr/hyprland.conf:

    exec-once = $BIN/scd
    bind = SUPER, F10, exec, $BIN/sc save

Reload Hyprland, approve the screen-share prompt once, then SUPER+F10 saves
your last N seconds. Open the app window any time with 'sc-gui'.

Ensure $BIN is on your PATH.
EOF

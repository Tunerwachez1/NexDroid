#!/usr/bin/env bash
# Installs the launcher entry and its icon for the current user.
#
# Wayland has no way to hand a window a picture: the compositor looks up the
# window's app_id in the desktop entries and takes the icon from there. So the
# entry and the icon theme are how liwinux gets an icon at all, and the app_id
# in the code has to match StartupWMClass here.
set -euo pipefail
R="$(cd "$(dirname "$0")/.." && pwd)"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icons="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"

install -Dm644 "$R/dist/desktop/liwinux.desktop" "$apps/liwinux.desktop"
# The game window has an app_id of its own; without an entry it has no icon.
install -Dm644 "$R/dist/desktop/liwinux-game.desktop" "$apps/liwinux-game.desktop"
install -Dm644 "$R/dist/desktop/nexdroid-desktop.desktop" "$apps/nexdroid-desktop.desktop"
install -Dm644 "$R/assets/waytrivo.png" "$icons/512x512/apps/waytrivo.png"
update-desktop-database "$apps" 2>/dev/null || true
gtk-update-icon-cache -f -t "$icons" 2>/dev/null || true
echo "WAYTRIVO shortcuts installed."

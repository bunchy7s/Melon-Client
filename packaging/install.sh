#!/bin/sh
# Puts the icon and the launcher entry where a Linux desktop looks for them,
# for the current user. Wayland shows a program's taskbar icon from this
# entry (named by the window's app id, `melon`), not from the window.
# Usage: packaging/install.sh [path of the melon binary]
set -e
here=$(cd "$(dirname "$0")" && pwd)
bin=${1:-$(cd "$here/.." && pwd)/target/release/melon}
data=${XDG_DATA_HOME:-$HOME/.local/share}
mkdir -p "$data/applications"
for size in 48 128 256; do
    dir="$data/icons/hicolor/${size}x${size}/apps"
    mkdir -p "$dir"
    cp "$here/melon$([ $size = 256 ] || echo "-$size").png" "$dir/melon-face.png"
done
sed "s|^Exec=.*|Exec=$bin|" "$here/melon.desktop" > "$data/applications/melon.desktop"
update-desktop-database "$data/applications" 2>/dev/null || true
gtk-update-icon-cache -f -q "$data/icons/hicolor" 2>/dev/null || true
echo "Installed: $data/applications/melon.desktop"

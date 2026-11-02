#!/usr/bin/env bash
# A nested headless Shell and temporary profile; never installs into the active session.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
shell_home=$(mktemp -d)
trap 'rm -rf "$shell_home"' EXIT
mkdir -p "$shell_home/data/gnome-shell/extensions" "$shell_home/config" "$shell_home/runtime"
chmod 700 "$shell_home/runtime"
cp -r "$root/extensions/spot-desktop@andrea.dev" "$shell_home/data/gnome-shell/extensions/"
env -u DISPLAY -u WAYLAND_DISPLAY HOME="$shell_home" XDG_DATA_HOME="$shell_home/data" \
    XDG_CONFIG_HOME="$shell_home/config" XDG_RUNTIME_DIR="$shell_home/runtime" \
    GSETTINGS_BACKEND=keyfile XDG_CURRENT_DESKTOP=GNOME GNOME_SHELL_SESSION_MODE=user \
    dbus-run-session --config-file="$root/scripts/test-session.conf" -- bash -c '
        set -euo pipefail
        root=$1
        gsettings set org.gnome.shell enabled-extensions "['\''spot-desktop@andrea.dev'\'']"
        gsettings set org.gnome.shell disable-user-extensions false
        gnome-shell --headless --wayland --no-x11 --virtual-monitor 1024x768 --wayland-display spot-test > "$HOME/shell.log" 2>&1 &
        shell_pid=$!
        trap '\''kill "$shell_pid" 2>/dev/null || true; wait "$shell_pid" 2>/dev/null || true'\'' EXIT
        for i in $(seq 1 100); do
            if gdbus call --session --dest dev.andrea.Spot.Desktop --object-path /dev/andrea/Spot/Desktop --method dev.andrea.Spot.Desktop.ListWindows >/dev/null 2>&1; then
                GDK_BACKEND=wayland WAYLAND_DISPLAY=spot-test GTK_A11Y=none GIO_USE_VFS=local python3 "$root/scripts/test-gnome.py"
                exit 0
            fi
            kill -0 "$shell_pid" 2>/dev/null || { tail -40 "$HOME/shell.log"; exit 1; }
            sleep .1
        done
        tail -40 "$HOME/shell.log"
        exit 1
    ' bash "$root"

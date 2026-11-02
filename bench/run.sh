#!/usr/bin/env bash
# A private bus and empty HOME keep the user's resident/configuration out of the run.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
launcher=$(realpath "${SPOT_BINARY:-$root/target/release/spot}")
[[ -x "$launcher" && -x "${launcher%/*}/spot-resident" ]] || {
    echo 'Build first: cargo build --release --locked' >&2; exit 1;
}
bench_home=$(mktemp -d)
trap 'rm -rf "$bench_home"' EXIT
mkdir -p "$bench_home/config" "$bench_home/data" "$bench_home/cache" "$bench_home/runtime"
chmod 700 "$bench_home/runtime"
env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS \
    HOME="$bench_home" XDG_CONFIG_HOME="$bench_home/config" \
    XDG_DATA_HOME="$bench_home/data" XDG_CACHE_HOME="$bench_home/cache" \
    XDG_RUNTIME_DIR="$bench_home/runtime" XDG_CURRENT_DESKTOP= \
    GTK_A11Y=none GIO_USE_VFS=local GDK_BACKEND=x11 GSK_RENDERER="${GSK_RENDERER:-gl}" \
    dbus-run-session --config-file="$root/scripts/test-session.conf" -- python3 "$root/bench/run.py" --binary "$launcher" "$@"

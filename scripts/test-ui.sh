#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Build without running tests; run GTK in a fresh process with an isolated environment.
test_binary=$(cargo test --locked --no-run --bin spot-resident --message-format=json | python3 -c 'import json,sys; artifacts=[json.loads(line) for line in sys.stdin]; print(next(a["executable"] for a in artifacts if a.get("reason")=="compiler-artifact" and a.get("executable") and a.get("profile",{}).get("test")))')
# init_translations() resolves the prefix from the executable. Test binaries live
# in <profile>/deps/, so their catalogs belong in <profile>/share/locale/.
test_prefix=$(dirname "$(dirname "$test_binary")")
for po in po/*.po; do
    lang=$(basename "$po" .po)
    catalog_dir="$test_prefix/share/locale/$lang/LC_MESSAGES"
    mkdir -p "$catalog_dir"
    msgfmt -c "$po" -o "$catalog_dir/spot.mo"
done
ui_home=$(mktemp -d)
trap 'rm -rf "$ui_home"' EXIT
mkdir -p "$ui_home/config" "$ui_home/data" "$ui_home/cache" "$ui_home/runtime"
chmod 700 "$ui_home/runtime"
xvfb-run -a env -u WAYLAND_DISPLAY HOME="$ui_home" XDG_CONFIG_HOME="$ui_home/config" \
    XDG_DATA_HOME="$ui_home/data" XDG_CACHE_HOME="$ui_home/cache" XDG_RUNTIME_DIR="$ui_home/runtime" \
    XDG_CURRENT_DESKTOP=Test GDK_BACKEND=x11 GSK_RENDERER=gl GTK_A11Y=none GIO_USE_VFS=local \
    dbus-run-session --config-file=scripts/test-session.conf -- "$test_binary" --ignored --test-threads=1 --nocapture

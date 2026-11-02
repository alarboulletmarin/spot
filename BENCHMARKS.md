# Reproducing Spot's footprint and latency

Build the release binaries, then run:

```bash
cargo build --release --locked
bench/run.sh --runs 20 > results.json
# SPOT_BINARY=/absolute/path/to/spot bench/run.sh --runs 50
```

The measurement tools are development-only: Bash, Python 3 with PyGObject/GIO
(`python-gobject` on Arch, `python3-gi` on Debian), `dbus-run-session`, Xvfb,
libX11, and optionally `glxinfo` (`mesa-utils`). They add no launcher dependencies.

The wrapper supplies an empty temporary HOME and XDG directories, an isolated
session bus with no service activation, a private Xvfb display, and X11/GL
rendering. It disables accessibility and uses local GIO file handling so host
portals, GVfs and accessibility services are not activated. The real desktop's
resident instance, clipboard, configuration and bus are untouched. Suggestions
and desktop modules keep their disabled defaults.

The JSON records distro, CPU, logical CPU count, kernel, renderer, GSK renderer,
version, Git revision/dirty state and binary SHA-256 digests, alongside raw samples:

- **Server ready:** process spawn to observing acquisition of the application
  D-Bus name (`NameOwnerChanged`). This is name acquisition, **not** completion
  of GTK/GSK initialization. The first window can still be substantially slower.
- **Cold window:** first activation of each newly started daemon to an actual
  X11 `MapNotify`, median over N fresh residents. This includes any remaining
  startup work after acquiring the name. It is not a cold filesystem-cache test.
- **Warm window:** launcher process spawn to X11 `MapNotify` on an existing
  resident, median over N activations. Each previous window must emit
  `UnmapNotify` after `spot --hide` before the next measurement. The extra hide
  control is not timed. IPC and observer/event-delivery overhead are included.
- **Idle RSS/PSS:** after the last window is hidden and a two-second wait, sum
  `/proc/<pid>/smaps_rollup` for the resident and all current descendants. Units
  are KiB; permission errors fail rather than undercount. System services,
  the bus, Xvfb and the measuring process are excluded.
- **Binary size:** both installed executables, individually and in total, in
  bytes. This is not the size of dynamically linked GTK/adwaita/libsoup/Mesa.

The script currently measures X11 under Xvfb. It does not claim to observe native
Wayland mapping: there is no portable cross-client MapNotify equivalent across
compositors. GPU/Wayland-session measurements require a compositor-specific
observer and must be labeled separately.

## Checkout measurement, 2 November 2026

[Raw output and binary hashes](bench/results/2026-11-02-arch-llvmpipe.json),
20 samples. Arch Linux, Intel i9-13900H / 20 logical CPUs, Mesa 26.2.6 llvmpipe,
private Xvfb, GL renderer, clean HOME. This measures the development changes for
issues #3–#10, **not** the published v0.5.1 tarball (the unchanged version string
is recorded with the dirty source revision in the artifact).

| Metric | Spot checkout |
|---|---:|
| Server D-Bus name acquisition, median | 46.73 ms |
| First activation of fresh daemon, median | 205.82 ms |
| Warm activation, median | 7.38 ms |
| Hidden idle RSS / PSS | 341.08 / 140.13 MiB |
| Resident processes | 1 |
| `spot` / `spot-resident` | 348,176 / 1,226,488 bytes |
| Total executable footprint | 1,574,664 bytes (1.50 MiB) |

RSS includes shared library/renderer pages; PSS apportions those pages. Software
rendering can substantially inflate both memory and frame time. These results
are specific to this host and methodology, not a performance guarantee. The
reported cold activation explicitly exposes startup work that a name-acquisition
measurement alone misses.

## Comparison supplied in issue #10

The following figures were supplied by the author in
[issue #10](https://github.com/alarboulletmarin/spot/issues/10), dated
2 November 2026. They are **not a comparison rerun** of this checkout; the original
raw samples and exact Spot revision were not provided. Units and scope below
retain the issue's labels and must not be mixed with the KiB/MiB measurements above.

Setup: Debian 13, 8 vCPU, no GPU, Xvfb, Mesa llvmpipe, empty HOME;
Spot release build versus Vicinae v0.29.2 AppImage.

| Metric | Spot (reported) | Vicinae (reported) |
|---|---:|---:|
| Idle RSS / PSS | 160 / 119 MB, 1 process | 315 / 254 MB, 4 processes |
| Binary size | 1.05 MB | 40 MB server, plus about 120 MB Node downloaded on first run |
| Server ready | 19–20 ms | 229–286 ms |
| Warm window open, median | 7.5–7.9 ms | 9.8–10.1 ms |

The issue's binary figure concerns the server; this repository's harness counts
both launcher and resident explicitly. llvmpipe under Xvfb inflates memory and
frame times for both applications. Real GPU/Wayland results will differ; treat
the supplied numbers as relative observations, not absolute claims.

Spot has no telemetry. Web suggestions remain an explicitly enabled network
feature; user-launched external programs have their own behavior. For comparison,
[Vicinae documents default system-information telemetry and an opt-out](https://docs.vicinae.com/telemetry).

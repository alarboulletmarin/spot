# spot

Application and file launcher for GNOME, in Rust + GTK4 / libadwaita (gtk4-rs).

Meant to be what Raycast is on macOS: a window that opens on a shortcut, filters as you type, and disappears as soon as you launch something.

## How it works

- **Applications** — enumerated with `Gio.AppInfo`, matched on name, generic name, keywords and executable, refreshed automatically when an application is installed or removed.
- **Files** — `plocate` queried asynchronously on each keystroke (90 ms debounce, from 3 characters), matched on the file name only, limited to your home directory, hidden entries and `node_modules` skipped.
- **Everything the Activities overview finds** — calculator results, Settings panels, Nautilus files: spot queries the same GNOME Shell search providers over D-Bus, and honours what you enabled in Settings → Search.
- **Commands** — when nothing matches and the first word is a program on your `PATH`, Enter runs the line as a command (in the background, no terminal).
- **System** — lock screen, suspend, log out, restart, shut down, over D-Bus; the last three go through GNOME's confirmation dialog.
- **Resident** — the first invocation stays in the background, with its window already created; every later `spot` is a small GIO-only binary that asks it over D-Bus to show that window, without loading GTK. On the author's machine that call takes about 10 ms (median), and the very first opening after login is as fast as the next ones (the window is built at startup instead of on first use, which used to cost ~1.9 s).

Ranking is a subsequence score: a literal match always wins, word starts get a bonus, shorter paths break ties. Applications you launch often get a bonus (counts in `~/.local/share/spot/usage.json`).

The interface follows the system language (English, French; add yours in `po/`). Application names and descriptions are already localized by GLib.

## Shortcuts

| Key | Action |
|---|---|
| `Super + Space` | open, or close when already open |
| `↑` `↓` | navigate |
| `Enter` | launch or open |
| `Ctrl + Enter` | open the folder containing the file |
| `Esc` | close |

The window also closes as soon as it loses focus.

## Supported platforms

Linux, on any distribution. The three building blocks are Linux-specific: applications come from `.desktop` files, files from `plocate`, and the resident process is woken over the D-Bus session bus. macOS and Windows are not supported and there is no plan for them: each of those three blocks would have to be rewritten, and Raycast already exists there.

Nothing in the code is tied to Arch; only the `PKGBUILD` is. Everywhere else it is `make install`.

| Environment | Status |
|---|---|
| GNOME, Wayland or X11 | first-class, this is what it is built and tested on |
| Any other GTK4 desktop (KDE Plasma, Cinnamon, Hyprland, Sway…) | should work, with the Adwaita look and a plain floating window; set the shortcut and a centering rule in your compositor. Not tested on those desktops: applications that are `OnlyShowIn=GNOME` are hidden (GLib's rule), and lock / log out / restart / shut down go through GNOME's session services, so they may do nothing |
| Arch Linux | `makepkg` from a checkout, see below; not on the AUR yet |
| Ubuntu 24.04+, Linux Mint 22+ | `make install` from a checkout, with a Rust installed through rustup. Build and install tested on Ubuntu 24.04 |
| Fedora, Debian 13, openSUSE Tumbleweed | `make install` from a checkout. Not tested; same Rust caveat |
| Debian 12, Ubuntu 22.04, Linux Mint 21 and older | no, GTK is older than 4.12 |

Requirements: GTK 4.12+, libadwaita 1, GLib, and `plocate` for file search. Building needs Rust **1.92 or newer** (`rust-version` in `Cargo.toml`, set by the GTK bindings). Most distributions package an older one (Ubuntu 24.04, hence Linux Mint 22, stops at 1.91), so install it with [rustup](https://rustup.rs) there; Arch is recent enough.

## Installation

Arch Linux: the PKGBUILD compiles the latest tagged release and installs it as the `spot-launcher` package, which pacman then tracks like any other. It is not on the AUR yet.

```bash
git clone https://github.com/alarboulletmarin/spot.git
cd spot
makepkg -si
```

Any other distribution, from a checkout. You need Rust 1.92+ (see above), the GTK 4 and libadwaita development files, and `gettext` for `msgfmt`:

```bash
# Debian, Ubuntu, Linux Mint
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev gettext plocate
# Fedora (names not tested)
sudo dnf install gcc pkgconf-pkg-config gtk4-devel libadwaita-devel gettext plocate

make                    # cargo build --release --locked
sudo make install       # PREFIX=/usr/local by default
```

Then log out and back in so the background service starts, or start it by hand once: `spot --daemon &`.

Set the keyboard shortcut in GNOME (Settings → Keyboard → Custom Shortcuts, command `spot`): an application cannot grab a global shortcut under Wayland.

Under Wayland a window cannot position itself; Mutter places it. To get it centered:

```bash
gsettings set org.gnome.mutter center-new-windows true
```

To upgrade, run the same commands again (`git pull` first on Arch), then restart the resident process: `spot --quit && spot --daemon &`.

## Appearance

Spot takes its colours from the desktop: light or dark follows the system setting, and so does the accent colour where libadwaita publishes it (libadwaita 1.6+, e.g. GNOME 47+; older versions use Adwaita blue). Like every libadwaita application it does not follow a custom GTK theme.

Pick a style in `~/.config/spot/spot.conf`. The file is watched: save it and the open window changes, no restart.

```ini
# Style: default, glass or compact
# ColorScheme: system, light or dark
[Appearance]
Style=glass
ColorScheme=system
```

Comments go on their own line: this format has no end-of-line comments.

| Style | Look |
|---|---|
| `default` | solid card, selected row filled with the accent colour |
| `glass` | translucent card, larger corners, selection as a soft tint of the accent colour. Needs a compositor; there is no blur, GTK cannot blur what is behind a window |
| `compact` | denser rows, smaller icons, an accent bar marks the selection |

For anything else, `~/.config/spot/style.css` is loaded after the built-in styles and reloaded the same way. libadwaita colours can be redefined, and the widgets have classes (`.spot-card`, `.spot-entry`, `.spot-row`, `.spot-icon`, `.spot-title`, `.spot-sub`, `.spot-kind`; they may change between versions):

```css
@define-color accent_bg_color #e66100;
.spot-card { border-radius: 8px; }
```

## Development

```bash
spot --quit; cargo run --release --bin spot-resident -- --daemon &   # run the checkout as the resident instance
make test                           # cargo test + translation files

SPOT_APP_ID=dev.andrea.SpotDev ...  # run next to the installed one (both `spot` and `spot-resident` read it)
```

A checkout runs in English: translations are compiled at install time. To add a language, copy `po/spot.pot` to `po/<lang>.po` and fill in the `msgstr` lines.

## Known limits

- No clipboard history and no window switcher: under Wayland a background application can neither watch the clipboard nor list windows.
- File search depends on the `plocate` database (`plocate-updatedb.timer`, daily by default); entries that no longer exist are hidden.

## License

MIT

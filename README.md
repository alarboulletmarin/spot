# spot

Application and file launcher for GNOME, in Python + GTK4 / libadwaita.

Meant to be what Raycast is on macOS: a window that opens on a shortcut, filters as you type, and disappears as soon as you launch something.

## How it works

- **Applications** — enumerated with `Gio.AppInfo`, matched on name, generic name, keywords and executable, refreshed automatically when an application is installed or removed.
- **Files** — `plocate` queried asynchronously on each keystroke (90 ms debounce, from 3 characters), matched on the file name only, limited to your home directory, hidden entries and `node_modules` skipped.
- **Everything the Activities overview finds** — calculator results, Settings panels, Nautilus files: spot queries the same GNOME Shell search providers over D-Bus, and honours what you enabled in Settings → Search.
- **Commands** — when nothing matches and the first word is a program on your `PATH`, Enter runs the line as a command (in the background, no terminal).
- **System** — lock screen, suspend, log out, restart, shut down, over D-Bus; the last three go through GNOME's confirmation dialog.
- **Resident** — the first invocation stays in the background; every later `spot` only asks it over D-Bus to show its window. That call goes through `gdbus` before Python loads GTK, so it takes about 30 ms instead of 200 ms.

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

Linux only. The three building blocks are Linux-specific: applications come from `.desktop` files, files from `plocate`, and the resident process is woken over the D-Bus session bus. There is no plan for macOS (Raycast is there) or Windows.

| Environment | Status |
|---|---|
| GNOME, Wayland or X11 | first-class, this is what it is built and tested on |
| Any other GTK4 desktop (KDE Plasma, Hyprland, Sway…) | works, with the Adwaita look and a plain floating window; set the shortcut and a centering rule in your compositor |
| Arch Linux | `makepkg` from a checkout, see below; not on the AUR yet |
| Fedora 40+, Ubuntu 24.04+, Debian 13+, openSUSE Tumbleweed | `make install` from a checkout |
| Debian 12 and older | no, GTK is older than 4.12 |

Requirements: Python 3.10+, PyGObject, GTK 4.12+, libadwaita 1, GLib (`gdbus`), and `plocate` for file search.

## Installation

Arch Linux: the PKGBUILD builds the latest tagged release and installs it as the `spot-launcher` package, which pacman then tracks like any other. It is not on the AUR yet.

```bash
git clone https://github.com/alarboulletmarin/spot.git
cd spot
makepkg -si
```

Any other distribution, from a checkout (needs `gettext` for `msgfmt`):

```bash
sudo make install       # PREFIX=/usr/local by default
```

Then log out and back in so the background service starts, or start it by hand once: `spot --daemon &`.

Set the keyboard shortcut in GNOME (Settings → Keyboard → Custom Shortcuts, command `spot`): an application cannot grab a global shortcut under Wayland.

Under Wayland a window cannot position itself; Mutter places it. To get it centered:

```bash
gsettings set org.gnome.mutter center-new-windows true
```

To upgrade, run the same commands again (`git pull` first on Arch), then restart the resident process: `spot --quit && spot --daemon &`.

## Development

```bash
spot --quit; ./spot.py --daemon &   # run the checkout as the resident instance
make test                           # unit tests + translation files
```

A checkout runs in English: translations are compiled at install time. To add a language, copy `po/spot.pot` to `po/<lang>.po` and fill in the `msgstr` lines.

## Known limits

- No clipboard history and no window switcher: under Wayland a background application can neither watch the clipboard nor list windows.
- File search depends on the `plocate` database (`plocate-updatedb.timer`, daily by default); entries that no longer exist are hidden.

## License

MIT

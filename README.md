# spot

Application and file launcher for GNOME, in Python + GTK4 / libadwaita.

Meant to be what Raycast is on macOS: a window that opens on a shortcut, filters as you type, and disappears as soon as you launch something.

## How it works

- **Applications** — enumerated with `Gio.AppInfo`, refreshed automatically when an application is installed or removed.
- **Files** — `plocate` queried asynchronously on each keystroke (90 ms debounce, from 3 characters), matched on the file name only, limited to your home directory, hidden entries and `node_modules` skipped.
- **Resident** — the first invocation stays in the background; every later `spot` only asks it over D-Bus to show its window. That call goes through `gdbus` before Python loads GTK, so it takes about 30 ms instead of 200 ms.

Ranking is a subsequence score: a literal match always wins, word starts get a bonus, shorter paths break ties.

## Shortcuts

| Key | Action |
|---|---|
| `Super + Space` | open |
| `↑` `↓` | navigate |
| `Enter` | launch or open |
| `Esc` | close |

The window also closes as soon as it loses focus.

## Installation

```bash
makepkg -f
sudo pacman -U spot-*.pkg.tar.zst
```

A background service starts with the session so the first opening is instant too; log out and back in, or start it by hand once: `spot --daemon &`.

Set the keyboard shortcut in GNOME (Settings → Keyboard → Custom Shortcuts, command `spot`): an application cannot grab a global shortcut under Wayland.

Under Wayland a window cannot position itself; Mutter places it. To get it centered:

```bash
gsettings set org.gnome.mutter center-new-windows true
```

After upgrading, restart the resident process: `spot --quit && spot --daemon &`.

## Development

```bash
./spot.py          # run from the checkout
python3 test_spot.py
```

## Known limits

- Applications and files only: no calculator, no clipboard history.
- File search depends on the `plocate` database (`plocate-updatedb.timer`, daily by default); entries that no longer exist are hidden.

## License

MIT

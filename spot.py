#!/usr/bin/env python3
"""spot — application and file launcher for GNOME.

The first invocation becomes a resident process; every later invocation only
asks it over D-Bus to show its window, so opening is instant.

Applications: Gio.AppInfo, refreshed whenever the desktop database changes.
Files:        plocate, queried asynchronously on each keystroke (debounced).
"""

from __future__ import annotations

import os
import stat
import sys

APP_ID = "dev.andrea.Spot"
OBJECT_PATH = "/dev/andrea/Spot"
VERSION = "0.2.0"


def wake_resident_instance() -> bool:
    """Ask the running instance to show its window, without loading GTK.

    Loading PyGObject + GTK + libadwaita costs ~200 ms; `gdbus` (shipped with
    GLib) does the same D-Bus call in ~10 ms. The activation token is forwarded
    so the window can take focus under Wayland.
    """
    platform_data = {
        key: os.environ[env]
        for key, env in (("activation-token", "XDG_ACTIVATION_TOKEN"),
                         ("desktop-startup-id", "DESKTOP_STARTUP_ID"))
        if os.environ.get(env)
    }
    if platform_data:
        args = "{" + ", ".join(f"'{k}': <{v!r}>" for k, v in platform_data.items()) + "}"
    else:
        args = "@a{sv} {}"
    argv = ["gdbus", "call", "--session", "--timeout", "2", "--dest", APP_ID,
            "--object-path", OBJECT_PATH, "--method",
            "org.freedesktop.Application.Activate", args]
    try:  # posix_spawnp rather than subprocess: importing subprocess alone costs ~20 ms
        pid = os.posix_spawnp(argv[0], argv, os.environ, file_actions=[
            (os.POSIX_SPAWN_OPEN, 1, os.devnull, os.O_WRONLY, 0),
            (os.POSIX_SPAWN_DUP2, 1, 2)])
    except OSError:
        return False  # gdbus missing
    return os.waitpid(pid, 0)[1] == 0


if __name__ == "__main__" and len(sys.argv) == 1 and wake_resident_instance():
    raise SystemExit(0)

import gettext  # noqa: E402
from dataclasses import dataclass  # noqa: E402

import gi  # noqa: E402

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, Gtk, Pango  # noqa: E402

if Gtk.check_version(4, 12, 0):  # None when the running GTK is recent enough
    raise SystemExit("spot needs GTK 4.12 or newer")

MAX_APPS = 8
MAX_FILES = 25
DEBOUNCE_MS = 90
MIN_FILE_QUERY = 3  # 2-letter queries yield ~100k plocate candidates (~0.5 s); 3 letters ~30k
HOME = os.path.expanduser("~")
# Translations sit next to the install prefix (/usr/bin/spot -> /usr/share/locale);
# a checkout has none and runs in English.
LOCALEDIR = os.path.join(os.path.dirname(os.path.dirname(os.path.realpath(__file__))),
                         "share", "locale")
_ = gettext.translation("spot", LOCALEDIR, fallback=True).gettext

CSS = """
window.spot { background-color: transparent; }
.spot-card {
    background-color: @window_bg_color;
    border-radius: 16px;
    border: 1px solid alpha(currentColor, 0.13);
}
.spot-entry {
    font-size: 1.35rem;
    padding: 16px 18px;
    background: none;
    border: none;
    box-shadow: none;
    min-height: 0;
}
.spot-sep { background-color: alpha(currentColor, 0.10); min-height: 1px; }
.spot-row { padding: 7px 10px; border-radius: 9px; }
.spot-sub { font-size: 0.82rem; opacity: 0.55; }
.spot-kind { font-size: 0.72rem; opacity: 0.45; }
"""


@dataclass(slots=True)
class Result:
    """One result row. `payload` is what gets launched."""

    score: int
    title: str
    subtitle: str
    kind: str
    gicon: Gio.Icon
    payload: Gio.AppInfo | str


def fuzzy_score(query: str, text: str) -> int:
    """Score a subsequence match. Returns -1 when there is no match."""
    q, t = query.lower(), text.lower()
    if not q:
        return 0
    idx = t.find(q)
    if idx >= 0:  # literal match always wins; a word start beats a mid-word hit
        word_start = idx == 0 or t[idx - 1] in " -_./"
        return 1000 - idx * 6 - len(t) // 4 + (30 if word_start else 0)
    score, pos, prev = 0, 0, -2
    for ch in q:
        pos = t.find(ch, pos)
        if pos < 0:
            return -1
        score += 12 if pos == prev + 1 else 1
        if pos == 0 or t[pos - 1] in " -_./":
            score += 9
        prev = pos
        pos += 1
    return score - len(t) // 12


def is_wanted_path(path: str) -> bool:
    """Keep paths inside $HOME, skipping hidden entries and node_modules."""
    if not path.startswith(HOME + "/"):
        return False
    rel = path[len(HOME):]  # starts with "/"
    return "/." not in rel and "/node_modules/" not in rel + "/"


def make_label(text: str, css: str | None = None) -> Gtk.Label:
    label = Gtk.Label(label=text, xalign=0, ellipsize=Pango.EllipsizeMode.END)
    if css:
        label.add_css_class(css)
    return label


class SpotWindow(Gtk.ApplicationWindow):
    def __init__(self, app: SpotApp):
        super().__init__(application=app, decorated=False, resizable=False)
        self.add_css_class("spot")
        self.set_default_size(720, -1)

        self._app = app
        self._results: list[Result] = []
        self._app_results: list[Result] = []
        self._generation = 0
        self._debounce_id = 0
        self._file_proc: Gio.Subprocess | None = None

        card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        card.add_css_class("spot-card")
        self.set_child(card)

        self.entry = Gtk.Entry(placeholder_text=_("Search applications and files…"))
        self.entry.add_css_class("spot-entry")
        self.entry.connect("changed", self._on_changed)
        self.entry.connect("activate", lambda *_: self._activate_selected())
        card.append(self.entry)

        self.sep = Gtk.Box(visible=False)
        self.sep.add_css_class("spot-sep")
        card.append(self.sep)

        self.list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.BROWSE)
        self.list.connect("row-activated", lambda _lb, row: self._activate_row(row))
        self.list.set_margin_start(8)
        self.list.set_margin_end(8)
        self.list.set_margin_bottom(8)

        self.scroller = Gtk.ScrolledWindow(
            visible=False,
            propagate_natural_height=True,
            max_content_height=430,
            hscrollbar_policy=Gtk.PolicyType.NEVER,
        )
        self.scroller.set_child(self.list)
        card.append(self.scroller)

        keys = Gtk.EventControllerKey()
        keys.connect("key-pressed", self._on_key)
        self.add_controller(keys)
        self.connect("notify::is-active", self._on_active_changed)

    # -- lifecycle -------------------------------------------------------

    def present_fresh(self) -> None:
        """Clear the query and show the window, ready for typing."""
        self.entry.set_text("")
        self._show_results([])
        self.present()
        self.entry.grab_focus()

    def _on_active_changed(self, *_args) -> None:
        if not self.is_active():  # focus lost: get out of the way
            self._dismiss()

    def _dismiss(self) -> None:
        self.set_visible(False)

    # -- keyboard --------------------------------------------------------

    def _on_key(self, _controller, keyval, _code, _state) -> bool:
        if keyval == Gdk.KEY_Escape:
            self._dismiss()
            return True
        if keyval in (Gdk.KEY_Down, Gdk.KEY_Up):
            self._move(1 if keyval == Gdk.KEY_Down else -1)
            return True
        return False

    def _move(self, delta: int) -> None:
        """Move the selection while keeping keyboard focus in the entry."""
        if not self._results:
            return
        current = self.list.get_selected_row()
        index = (current.get_index() if current else -1) + delta
        index = max(0, min(index, len(self._results) - 1))
        row = self.list.get_row_at_index(index)
        if row is None:
            return
        self.list.select_row(row)
        _ok, bounds = row.compute_bounds(self.list)
        self.scroller.get_vadjustment().clamp_page(
            bounds.get_y(), bounds.get_y() + bounds.get_height())

    # -- search ----------------------------------------------------------

    def _on_changed(self, _entry) -> None:
        if self._debounce_id:
            GLib.source_remove(self._debounce_id)
        self._debounce_id = GLib.timeout_add(DEBOUNCE_MS, self._search)

    def _search(self) -> bool:
        self._debounce_id = 0
        self._generation += 1
        if self._file_proc is not None:  # a newer keystroke supersedes it
            self._file_proc.force_exit()
            self._file_proc = None
        query = self.entry.get_text().strip()

        if not query:
            self._app_results = []
            self._show_results([])
            return GLib.SOURCE_REMOVE

        self._app_results = self._search_apps(query)[:MAX_APPS]
        self._show_results(self._app_results)  # show right away, files follow

        if len(query) >= MIN_FILE_QUERY:
            self._search_files(query, self._generation)
        return GLib.SOURCE_REMOVE

    def _search_apps(self, query: str) -> list[Result]:
        results = []
        for info in self._app.apps:
            name = info.get_display_name()
            score = fuzzy_score(query, name)
            if score < 0 and (generic := info.get_generic_name()):
                score = fuzzy_score(query, generic)
                if score >= 0:
                    score -= 40  # less direct than a match on the name
            if score >= 0:
                icon = info.get_icon() or Gio.ThemedIcon.new("application-x-executable")
                results.append(Result(score, name, info.get_description() or "",
                                      _("Application"), icon, info))
        results.sort(key=lambda r: -r.score)
        return results

    def _search_files(self, query: str, generation: int) -> None:
        # --basename: match the file name only; the whole path would surface
        # every file under a matching directory. No --limit: plocate lists
        # paths in sorted order, so a limit would be spent on /etc and ~/.cache
        # before reaching anything we want to show.
        try:
            self._file_proc = Gio.Subprocess.new(
                ["plocate", "--ignore-case", "--basename", "--", query],
                Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_SILENCE,
            )
        except GLib.Error:
            return  # plocate not installed: applications only
        self._file_proc.communicate_utf8_async(None, None, self._on_files_ready,
                                               (generation, query))

    def _on_files_ready(self, proc, result, data) -> None:
        generation, query = data
        if generation != self._generation:
            return  # superseded by a newer keystroke
        self._file_proc = None
        try:
            ok, stdout, _stderr = proc.communicate_utf8_finish(result)
        except GLib.Error:
            return
        if not ok or not stdout:
            return

        candidates = [p for p in stdout.splitlines() if is_wanted_path(p)]
        candidates.sort(key=lambda p: (-fuzzy_score(query, os.path.basename(p)), len(p)))

        files: list[Result] = []
        for path in candidates:
            try:
                is_dir = stat.S_ISDIR(os.stat(path).st_mode)
            except OSError:
                continue  # stale index entry
            name = os.path.basename(path)
            content_type = "inode/directory" if is_dir else Gio.content_type_guess(name, None)[0]
            files.append(Result(fuzzy_score(query, name), name,
                                path.replace(HOME, "~", 1), _("Folder") if is_dir else _("File"),
                                Gio.content_type_get_icon(content_type), path))
            if len(files) >= MAX_FILES:
                break
        self._show_results(self._app_results + files)

    # -- display ---------------------------------------------------------

    def _show_results(self, results: list[Result]) -> None:
        self._results = results
        self.list.remove_all()

        for item in results:
            row = Gtk.ListBoxRow()
            row.add_css_class("spot-row")

            box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
            box.append(Gtk.Image.new_from_gicon(item.gicon))

            texts = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, hexpand=True)
            texts.append(make_label(item.title))
            if item.subtitle:
                texts.append(make_label(item.subtitle, "spot-sub"))
            box.append(texts)

            kind = Gtk.Label(label=item.kind)
            kind.add_css_class("spot-kind")
            box.append(kind)

            row.set_child(box)
            self.list.append(row)

        visible = bool(results)
        self.scroller.set_visible(visible)
        self.sep.set_visible(visible)
        if visible:
            self.list.select_row(self.list.get_row_at_index(0))

    # -- launching -------------------------------------------------------

    def _activate_selected(self) -> None:
        self._activate_row(self.list.get_selected_row())

    def _activate_row(self, row: Gtk.ListBoxRow | None) -> None:
        if row is None or row.get_index() >= len(self._results):
            return
        item = self._results[row.get_index()]
        context = self.get_display().get_app_launch_context()
        try:
            if isinstance(item.payload, str):
                uri = Gio.File.new_for_path(item.payload).get_uri()
                Gio.AppInfo.launch_default_for_uri(uri, context)
            else:
                item.payload.launch(None, context)
        except GLib.Error as error:
            print("spot: " + _("Launch failed: %s") % error.message, file=sys.stderr)
        self._dismiss()


class SpotApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id=APP_ID,
                         flags=Gio.ApplicationFlags.HANDLES_COMMAND_LINE)
        self.apps: list[Gio.AppInfo] = []
        self.window: SpotWindow | None = None

    def do_startup(self) -> None:
        Adw.Application.do_startup(self)
        self.hold()  # stay resident: dismissing only hides the window
        provider = Gtk.CssProvider()
        provider.load_from_string(CSS)
        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(), provider,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
        self._app_monitor = Gio.AppInfoMonitor.get()  # keep a reference or the signal is lost
        self._app_monitor.connect("changed", lambda *_: self._load_apps())
        self._load_apps()

    def _load_apps(self) -> None:
        self.apps = [app for app in Gio.AppInfo.get_all() if app.should_show()]

    def do_command_line(self, command_line) -> int:
        args = command_line.get_arguments()
        if "--version" in args:
            command_line.print_literal(f"spot {VERSION}\n")
        elif "--quit" in args:
            self.quit()
        elif "--daemon" not in args:  # --daemon: start resident without showing the window
            self.activate()
        return 0

    def do_activate(self) -> None:
        if self.window is None:
            self.window = SpotWindow(self)
        self.window.present_fresh()


def main() -> int:
    return SpotApp().run(sys.argv)


if __name__ == "__main__":
    raise SystemExit(main())

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const BUS = 'dev.andrea.Spot.Desktop';
const PATH = '/dev/andrea/Spot/Desktop';
const XML = `<node><interface name="dev.andrea.Spot.Desktop">
<method name="ListWindows"><arg type="a(tss)" direction="out"/></method>
<method name="FocusWindow"><arg type="t" direction="in"/></method>
<method name="SetClipboardEnabled"><arg type="b" direction="in"/></method>
<signal name="ClipboardChanged"><arg type="s"/><arg type="as"/></signal>
</interface></node>`;

export default class SpotDesktop extends Extension {
    enable() {
        this._clients = new Map();
        this._selection = global.display.get_selection();
        this._generation = 0;
        this._object = Gio.DBusExportedObject.wrapJSObject(XML, this);
        this._object.export(Gio.DBus.session, PATH);
        this._owner = Gio.bus_own_name_on_connection(Gio.DBus.session, BUS,
            Gio.BusNameOwnerFlags.NONE, null, null);
    }

    _windows() {
        return global.get_window_actors().map(actor => actor.meta_window)
            .filter(window => !window.skip_taskbar);
    }

    ListWindows() {
        return this._windows().map(window => [window.get_id(), window.get_title() ?? '',
            window.get_gtk_application_id() ?? window.get_wm_class() ?? '']);
    }

    FocusWindow(id) {
        const window = this._windows().find(w => w.get_id() === id);
        if (window) {
            window.get_workspace()?.activate(global.get_current_time());
            window.activate(global.get_current_time());
        }
    }

    SetClipboardEnabledAsync([enabled], invocation) {
        const sender = invocation.get_sender();
        if (enabled && !this._clients.has(sender)) {
            const watch = Gio.bus_watch_name_on_connection(Gio.DBus.session, sender,
                Gio.BusNameWatcherFlags.NONE, null, () => this._removeClient(sender));
            this._clients.set(sender, watch);
        } else if (!enabled) {
            this._removeClient(sender);
        }
        this._syncWatch();
        invocation.return_value(null);
    }

    _removeClient(sender) {
        const watch = this._clients.get(sender);
        if (watch) {
            Gio.bus_unwatch_name(watch);
            this._clients.delete(sender);
        }
        this._syncWatch();
    }

    _syncWatch() {
        if (this._clients.size && !this._signal) {
            this._signal = this._selection.connect('owner-changed', (_selection, type) => {
                if (type === Meta.SelectionType.SELECTION_CLIPBOARD)
                    this._read();
            });
        } else if (!this._clients.size && this._signal) {
            this._selection.disconnect(this._signal);
            this._signal = null;
            this._cancel?.cancel();
            this._generation++;
        }
    }

    _read() {
        this._cancel?.cancel();
        const generation = ++this._generation;
        const type = Meta.SelectionType.SELECTION_CLIPBOARD;
        const mimes = this._selection.get_mimetypes(type) ?? [];
        if (mimes.some(m => /password|secret|keepass/i.test(m)))
            return;
        const mime = ['text/plain;charset=utf-8', 'text/plain'].find(m => mimes.includes(m));
        if (!mime)
            return;
        this._cancel = new Gio.Cancellable();
        const stream = Gio.MemoryOutputStream.new_resizable();
        // Mutter caps the transfer, rather than reading an unbounded selection into Shell.
        this._selection.transfer_async(type, mime, 16385, stream, this._cancel,
            (selection, result) => {
                try {
                    selection.transfer_finish(result);
                    stream.close(null);
                    const bytes = stream.steal_as_bytes().get_data();
                    if (generation !== this._generation || !this._clients.size || bytes.length > 16384)
                        return;
                    const text = new TextDecoder('utf-8', {fatal: true}).decode(bytes);
                    if (text.trim())
                        this._object.emit_signal('ClipboardChanged', new GLib.Variant('(sas)', [text, mimes]));
                } catch (_) {
                    // Cancellation, unsupported content and invalid UTF-8 are intentionally silent.
                }
            });
    }

    disable() {
        this._cancel?.cancel();
        this._generation++;
        if (this._signal)
            this._selection.disconnect(this._signal);
        this._signal = null;
        for (const watch of this._clients.values())
            Gio.bus_unwatch_name(watch);
        this._clients.clear();
        this._object?.unexport();
        if (this._owner)
            Gio.bus_unown_name(this._owner);
        this._owner = null;
        this._object = null;
        this._selection = null;
    }
}

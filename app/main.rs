//! spot-resident — the GTK application behind `spot` (see spot.rs).
//!
//! The first invocation becomes a resident process; every later `spot` only
//! asks it over D-Bus to show its window, so opening is instant.
//!
//! Applications: Gio.AppInfo, refreshed whenever the desktop database changes.
//! Files:        plocate, queried asynchronously on each keystroke (debounced).
//! Everything else (calculator, settings panels…): the GNOME Shell search
//! providers, the same D-Bus services the Activities overview queries.

mod providers;
mod results;
mod search;
mod ui;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::OnceCell;
use std::ffi::{CString, c_char};
use std::rc::Rc;

unsafe extern "C" {
    fn bindtextdomain(domain: *const c_char, dir: *const c_char) -> *mut c_char;
    fn bind_textdomain_codeset(domain: *const c_char, codeset: *const c_char) -> *mut c_char;
}

/// Translate through the "spot" catalog; the msgid itself when there is none.
pub fn tr(msgid: &str) -> String {
    glib::dgettext(Some("spot"), msgid).into()
}

/// Translations sit next to the install prefix (/usr/bin/spot -> /usr/share/locale);
/// a checkout has none and runs in English.
fn init_translations() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(prefix) = exe.parent().and_then(|bin| bin.parent()) else {
        return;
    };
    let dir = CString::new(prefix.join("share/locale").to_string_lossy().as_bytes()).unwrap();
    // SAFETY: plain libc calls with NUL-terminated strings that outlive them.
    unsafe {
        bindtextdomain(c"spot".as_ptr(), dir.as_ptr());
        bind_textdomain_codeset(c"spot".as_ptr(), c"UTF-8".as_ptr());
    }
}

/// $SPOT_APP_ID lets a development build run next to the installed one (same in spot.rs).
fn app_id() -> String {
    std::env::var("SPOT_APP_ID").unwrap_or_else(|_| "dev.andrea.Spot".into())
}

fn main() -> glib::ExitCode {
    init_translations();

    let app = adw::Application::builder()
        .application_id(app_id())
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let window: Rc<OnceCell<Rc<ui::Ui>>> = Rc::default();

    let w = window.clone();
    app.connect_startup(move |app| {
        std::mem::forget(app.hold()); // stay resident: dismissing only hides the window
        let provider = gtk::CssProvider::new();
        provider.load_from_string(ui::CSS);
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().expect("no display"),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let data = ui::AppData::new(app.dbus_connection());
        let monitor = gio::AppInfoMonitor::get();
        let d = data.clone();
        monitor.connect_changed(move |_| d.load_apps());
        std::mem::forget(monitor); // keep a reference or the signal is lost

        let ui = ui::Ui::new(app, data);
        // Creating the window (and with it the GSK renderer) on the first show cost ~1.9 s
        // after login; doing it here, unmapped, makes the first show as fast as the rest.
        WidgetExt::realize(&ui.win);
        let _ = w.set(ui);
    });

    app.connect_command_line(|app, command_line| {
        let args = command_line.arguments();
        if args.iter().any(|a| a == "--quit") {
            app.quit();
        } else if !args.iter().any(|a| a == "--daemon") {
            app.activate(); // --daemon: start resident without showing the window
        }
        glib::ExitCode::SUCCESS
    });

    app.connect_activate(move |_| {
        let Some(ui) = window.get() else { return };
        if ui.win.is_active() {
            ui.dismiss(); // the shortcut pressed while open: close
        } else {
            ui.present_fresh();
        }
    });

    app.run()
}

#[cfg(test)]
mod tests {
    /// Every tr("…") in the sources must have a msgid in every catalog.
    #[test]
    fn translations_cover_source_strings() {
        let sources = [
            include_str!("main.rs"),
            include_str!("providers.rs"),
            include_str!("results.rs"),
            include_str!("ui.rs"),
        ];
        let ids: Vec<&str> = sources
            .iter()
            .flat_map(|s| s.split("tr(\"").skip(1))
            .filter_map(|s| s.split_once("\")").map(|(id, _)| id))
            .filter(|id| !id.contains('"') && *id != "…") // "…" is this test's own doc comment
            .collect();
        assert!(!ids.is_empty(), "no translatable strings found");
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/po")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "po") {
                let po = std::fs::read_to_string(&path).unwrap();
                for id in &ids {
                    assert!(
                        po.contains(&format!("msgid \"{id}\"")),
                        "{path:?}: missing {id}"
                    );
                }
            }
        }
    }
}

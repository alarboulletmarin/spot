//! Opt-in desktop sources. With both flags off there is no backend, thread, timer or watch.
use crate::{
    config,
    results::{Hit, Then, themed},
    search::fuzzy_score,
    tr,
};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

pub const BUS: &str = "dev.andrea.Spot.Desktop";
pub const PATH: &str = "/dev/andrea/Spot/Desktop";
pub const IFACE: &str = "dev.andrea.Spot.Desktop";
#[derive(Clone, Debug)]
pub struct Window {
    pub id: u64,
    pub title: String,
    pub app: String,
}
#[derive(Default)]
pub struct Snapshot {
    pub windows: Vec<Window>,
    pub clipboard: VecDeque<String>,
    pub windows_available: bool,
    pub clipboard_available: bool,
}
impl Snapshot {
    pub fn remember(&mut self, text: String) {
        if text.trim().is_empty() || text.len() > 16384 {
            return;
        }
        self.clipboard.retain(|s| s != &text);
        self.clipboard.push_front(text);
        self.clipboard.truncate(50);
    }
}
pub fn sensitive(mimes: &[String]) -> bool {
    mimes.iter().any(|m| {
        let m = m.to_ascii_lowercase();
        m.contains("password")
            || m.contains("secret")
            || m.contains("keepass")
            || m.contains("x-kde-passwordmanagerhint")
    })
}
struct Backend {
    flags: (bool, bool),
    snapshot: Arc<Mutex<Snapshot>>,
    stop: Arc<AtomicBool>,
    focus: Option<mpsc::Sender<u64>>,
    bus: Option<gio::DBusConnection>,
    subscription: RefCell<Option<gio::SignalSubscription>>,
    timer: RefCell<Option<glib::SourceId>>,
}
impl Drop for Backend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.subscription.take();
        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
        if let Some(bus) = &self.bus {
            shell_call(
                bus,
                "SetClipboardEnabled",
                Some((false,).to_variant()),
                |_| {},
            );
        }
    }
}
thread_local! { static RESIDENT: Cell<bool> = const { Cell::new(false) }; static BACKEND: RefCell<Option<Rc<Backend>>> = const { RefCell::new(None) }; }

pub fn init() {
    RESIDENT.set(true);
    configure();
}

pub fn configure() {
    if !RESIDENT.get() {
        return;
    }
    let c = config::current();
    let flags = (c.clipboard_enabled, c.windows_enabled);
    BACKEND.with_borrow_mut(|slot| {
        if slot.as_ref().is_some_and(|b| b.flags == flags) {
            return;
        }
        *slot = None;
        if flags == (false, false) {
            return;
        }
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let gnome = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_uppercase()
            .contains("GNOME");
        let (focus, bus) = if gnome {
            (
                None,
                gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok(),
            )
        } else {
            let (tx, rx) = mpsc::channel();
            crate::desktop_wayland::start(flags, snapshot.clone(), stop.clone(), rx);
            (Some(tx), None)
        };
        let backend = Rc::new(Backend {
            flags,
            snapshot,
            stop,
            focus,
            bus,
            subscription: RefCell::default(),
            timer: RefCell::default(),
        });
        if let Some(bus) = &backend.bus {
            if flags.0 {
                let snapshot = backend.snapshot.clone();
                *backend.subscription.borrow_mut() = Some(bus.subscribe_to_signal(
                    Some(BUS),
                    Some(IFACE),
                    Some("ClipboardChanged"),
                    Some(PATH),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |signal| {
                        if let Some((text, mimes)) =
                            signal.parameters.get::<(String, Vec<String>)>()
                            && !sensitive(&mimes)
                        {
                            snapshot.lock().unwrap().remember(text);
                        }
                    },
                ));
            }
            backend.refresh_shell();
            let weak = Rc::downgrade(&backend);
            *backend.timer.borrow_mut() = Some(glib::timeout_add_local(
                std::time::Duration::from_secs(2),
                move || {
                    let Some(b) = weak.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    b.refresh_shell();
                    glib::ControlFlow::Continue
                },
            ));
        }
        *slot = Some(backend);
    });
}
fn shell_call(
    bus: &gio::DBusConnection,
    method: &str,
    args: Option<glib::Variant>,
    callback: impl FnOnce(Result<glib::Variant, glib::Error>) + 'static,
) {
    bus.call(
        Some(BUS),
        PATH,
        IFACE,
        method,
        args.as_ref(),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        1000,
        gio::Cancellable::NONE,
        callback,
    );
}
impl Backend {
    fn refresh_shell(&self) {
        if let Some(bus) = &self.bus {
            if self.flags.0 {
                let s = self.snapshot.clone();
                shell_call(
                    bus,
                    "SetClipboardEnabled",
                    Some((true,).to_variant()),
                    move |r| {
                        s.lock().unwrap().clipboard_available = r.is_ok();
                    },
                );
            }
            if self.flags.1 {
                let s = self.snapshot.clone();
                shell_call(bus, "ListWindows", None, move |r| {
                    let windows = r
                        .ok()
                        .and_then(|v| v.get::<(Vec<(u64, String, String)>,)>());
                    let mut snapshot = s.lock().unwrap();
                    snapshot.windows_available = windows.is_some();
                    snapshot.windows = windows
                        .map(|(w,)| {
                            w.into_iter()
                                .map(|(id, title, app)| Window { id, title, app })
                                .collect()
                        })
                        .unwrap_or_default();
                });
            }
        }
    }
}

pub fn rows(query: &str) -> Option<Vec<Hit>> {
    let (clipboard, filter) = if let Some(q) = query.strip_prefix("clipboard:") {
        (true, q.trim())
    } else {
        (false, query.strip_prefix("windows:")?.trim())
    };
    configure();
    Some(BACKEND.with_borrow(|slot| {
        let enabled = config::current();
        if if clipboard { !enabled.clipboard_enabled } else { !enabled.windows_enabled } {
            return vec![Hit { title: tr("Module disabled"), subtitle: tr("Enable it explicitly in spot.conf"), then: Then::Query(">".into()), ..Default::default() }];
        }
        let Some(b) = slot else { return vec![]; };
        let snapshot = b.snapshot.lock().unwrap();
        let available = if clipboard { snapshot.clipboard_available } else { snapshot.windows_available };
        if !available { return vec![Hit { title: tr("Desktop backend unavailable"), subtitle: tr("Enable the Spot GNOME extension or use a compositor supporting the required Wayland protocols"), ..Default::default() }]; }
        let mut hits = vec![];
        if clipboard {
            for text in &snapshot.clipboard {
                if fuzzy_score(filter,text) < 0 { continue; }
                let text = text.clone();
                hits.push(Hit { title: text.lines().next().unwrap_or("").to_owned(), kind: tr("Clipboard"), verb: tr("Copy"),
                    activate: Box::new(move |_| { if let Some(d) = gdk::Display::default() { d.clipboard().set_text(&text); } Ok(()) }), ..Default::default() });
            }
            hits.push(Hit { title: tr("Clear clipboard history"), icon: themed("edit-clear-symbolic"), verb: tr("Clear"),
                activate: Box::new(|_| { BACKEND.with_borrow(|b| { if let Some(b) = b { b.snapshot.lock().unwrap().clipboard.clear(); } }); Ok(()) }), then: Then::Refresh, ..Default::default() });
        } else {
            for window in &snapshot.windows {
                if fuzzy_score(filter,&format!("{} {}",window.title,window.app)) < 0 { continue; }
                let (id,bus,focus) = (window.id,b.bus.clone(),b.focus.clone());
                hits.push(Hit { title: window.title.clone(), subtitle: window.app.clone(), kind: tr("Window"), verb: tr("Focus"),
                    activate: Box::new(move |_| {
                        if let Some(bus) = &bus { shell_call(bus,"FocusWindow",Some((id,).to_variant()), |_| {}); }
                        if let Some(tx) = &focus { let _ = tx.send(id); } Ok(())
                    }), ..Default::default() });
            }
            if hits.is_empty() { hits.push(Hit { title: tr("No windows"), ..Default::default() }); }
        }
        hits
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_history_and_sensitive_hints() {
        let mut s = Snapshot::default();
        for i in 0..60 {
            s.remember(i.to_string());
        }
        assert_eq!(s.clipboard.len(), 50);
        s.remember("59".into());
        assert_eq!(s.clipboard.len(), 50);
        assert_eq!(s.clipboard[0], "59");
        s.remember("x".repeat(16385));
        assert_eq!(s.clipboard[0], "59");
        assert!(sensitive(&["x-kde-passwordManagerHint".into()]));
        assert!(sensitive(&["application/x-keepassxc".into()]));
        assert!(!sensitive(&["text/plain".into()]));
    }
}

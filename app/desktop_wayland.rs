//! Native Wayland backend, constructed on a worker only when a desktop module is enabled.
use crate::desktop::{Snapshot, Window, sensitive};
use std::{
    collections::HashMap,
    io::Read,
    os::{fd::AsFd, unix::net::UnixStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
    },
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, delegate_noop,
    protocol::{wl_registry, wl_seat},
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1 as cd, ext_data_control_manager_v1 as cm,
    ext_data_control_offer_v1 as co, ext_data_control_source_v1 as cs,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1 as wh, zwlr_foreign_toplevel_manager_v1 as wm,
};

pub fn start(
    flags: (bool, bool),
    snapshot: Arc<Mutex<Snapshot>>,
    stop: Arc<AtomicBool>,
    focus: Receiver<u64>,
) {
    std::thread::spawn(move || {
        let Ok(conn) = Connection::connect_to_env() else {
            return;
        };
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut state = State {
            flags,
            snapshot: snapshot.clone(),
            seat: None,
            clipboard: None,
            device: None,
            windows: HashMap::new(),
            offers: HashMap::new(),
            pending: None,
        };
        while !stop.load(Ordering::Relaxed) {
            if queue.roundtrip(&mut state).is_err() {
                break;
            }
            for id in focus.try_iter() {
                if let (Some((handle, _)), Some(seat)) = (state.windows.get(&id), &state.seat) {
                    handle.activate(seat);
                }
            }
            state.read_clipboard();
            snapshot.lock().unwrap().windows =
                state.windows.values().map(|(_, w)| w.clone()).collect();
            std::thread::sleep(Duration::from_millis(100));
        }
        let mut s = snapshot.lock().unwrap();
        s.windows_available = false;
        s.clipboard_available = false;
    });
}
struct Pending {
    stream: UnixStream,
    bytes: Vec<u8>,
    since: Instant,
}
struct State {
    flags: (bool, bool),
    snapshot: Arc<Mutex<Snapshot>>,
    seat: Option<wl_seat::WlSeat>,
    clipboard: Option<cm::ExtDataControlManagerV1>,
    device: Option<cd::ExtDataControlDeviceV1>,
    windows: HashMap<u64, (wh::ZwlrForeignToplevelHandleV1, Window)>,
    offers: HashMap<u32, (co::ExtDataControlOfferV1, Vec<String>)>,
    pending: Option<Pending>,
}
impl State {
    fn bind_clipboard(&mut self, qh: &QueueHandle<Self>) {
        if self.device.is_none()
            && let (Some(manager), Some(seat)) = (&self.clipboard, &self.seat)
        {
            self.device = Some(manager.get_data_device(seat, qh, ()));
            self.snapshot.lock().unwrap().clipboard_available = true;
        }
    }
    fn read_clipboard(&mut self) {
        let Some(mut pending) = self.pending.take() else {
            return;
        };
        if pending.since.elapsed() > Duration::from_secs(2) {
            return;
        }
        let mut buf = [0u8; 4096];
        loop {
            match pending.stream.read(&mut buf) {
                Ok(0) => {
                    if let Ok(text) = String::from_utf8(pending.bytes) {
                        self.snapshot.lock().unwrap().remember(text);
                    }
                    return;
                }
                Ok(n) => {
                    pending.bytes.extend_from_slice(&buf[..n]);
                    if pending.bytes.len() > 16384 {
                        return;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    self.pending = Some(pending);
                    return;
                }
                Err(_) => return,
            }
        }
    }
}
impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        s: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" if s.seat.is_none() => {
                    s.seat = Some(registry.bind(name, version.min(7), qh, ()));
                    s.bind_clipboard(qh);
                }
                "ext_data_control_manager_v1" if s.flags.0 => {
                    s.clipboard = Some(registry.bind(name, 1, qh, ()));
                    s.bind_clipboard(qh);
                }
                "zwlr_foreign_toplevel_manager_v1" if s.flags.1 => {
                    registry.bind::<wm::ZwlrForeignToplevelManagerV1, _, _>(
                        name,
                        version.min(3),
                        qh,
                        (),
                    );
                    s.snapshot.lock().unwrap().windows_available = true;
                }
                _ => {}
            }
        }
    }
}
impl Dispatch<wm::ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        s: &mut Self,
        _: &wm::ZwlrForeignToplevelManagerV1,
        event: wm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wm::Event::Toplevel { toplevel } => {
                let id = toplevel.id().protocol_id() as u64;
                s.windows.insert(
                    id,
                    (
                        toplevel,
                        Window {
                            id,
                            title: String::new(),
                            app: String::new(),
                        },
                    ),
                );
            }
            wm::Event::Finished => {
                s.windows.clear();
                s.snapshot.lock().unwrap().windows_available = false;
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(State,wm::ZwlrForeignToplevelManagerV1,[0=>(wh::ZwlrForeignToplevelHandleV1,())]);
}
impl Dispatch<wh::ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        s: &mut Self,
        handle: &wh::ZwlrForeignToplevelHandleV1,
        event: wh::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = handle.id().protocol_id() as u64;
        match event {
            wh::Event::Title { title } => {
                if let Some((_, w)) = s.windows.get_mut(&id) {
                    w.title = title;
                }
            }
            wh::Event::AppId { app_id } => {
                if let Some((_, w)) = s.windows.get_mut(&id) {
                    w.app = app_id;
                }
            }
            wh::Event::Closed => {
                s.windows.remove(&id);
                handle.destroy();
            }
            _ => {}
        }
    }
}
impl Dispatch<cd::ExtDataControlDeviceV1, ()> for State {
    fn event(
        s: &mut Self,
        _: &cd::ExtDataControlDeviceV1,
        event: cd::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            cd::Event::DataOffer { id } => {
                s.offers.insert(id.id().protocol_id(), (id, vec![]));
            }
            cd::Event::Selection { id } => {
                s.pending = None;
                if let Some(id) = id
                    && let Some((offer, mimes)) = s.offers.get(&id.id().protocol_id())
                    && !sensitive(mimes)
                    && let Some(mime) = mimes
                        .iter()
                        .find(|m| m.as_str() == "text/plain;charset=utf-8")
                        .or_else(|| mimes.iter().find(|m| m.as_str() == "text/plain"))
                    && let Ok((read, write)) = UnixStream::pair()
                {
                    let _ = read.set_nonblocking(true);
                    offer.receive(mime.clone(), write.as_fd());
                    s.pending = Some(Pending {
                        stream: read,
                        bytes: vec![],
                        since: Instant::now(),
                    });
                }
                for (_, (offer, _)) in s.offers.drain() {
                    offer.destroy();
                }
            }
            cd::Event::Finished => {
                s.snapshot.lock().unwrap().clipboard_available = false;
            }
            cd::Event::PrimarySelection { id: Some(offer) } => {
                if let Some((offer, _)) = s.offers.remove(&offer.id().protocol_id()) {
                    offer.destroy();
                }
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(State,cd::ExtDataControlDeviceV1,[0=>(co::ExtDataControlOfferV1,())]);
}
impl Dispatch<co::ExtDataControlOfferV1, ()> for State {
    fn event(
        s: &mut Self,
        offer: &co::ExtDataControlOfferV1,
        event: co::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let co::Event::Offer { mime_type } = event
            && let Some((_, mimes)) = s.offers.get_mut(&offer.id().protocol_id())
        {
            mimes.push(mime_type);
        }
    }
}
delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ignore cm::ExtDataControlManagerV1);
delegate_noop!(State: ignore cs::ExtDataControlSourceV1);

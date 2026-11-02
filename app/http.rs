//! Minimal libsoup 3 adapter: native GLib async I/O, cancellation and TLS without a runtime.
//! No cookie jar, disk cache, logger or authentication handler is installed.
use gio::glib::{self, translate::*};
use gio::prelude::*;
use std::ffi::{CString, c_char, c_void};

#[link(name = "soup-3.0")]
unsafe extern "C" {
    fn soup_session_new() -> *mut glib::gobject_ffi::GObject;
    fn soup_message_new(
        method: *const c_char,
        uri: *const c_char,
    ) -> *mut glib::gobject_ffi::GObject;
    fn soup_message_set_flags(message: *mut glib::gobject_ffi::GObject, flags: u32);
    fn soup_message_get_status(message: *mut glib::gobject_ffi::GObject) -> u32;
    fn soup_session_send_async(
        session: *mut glib::gobject_ffi::GObject,
        message: *mut glib::gobject_ffi::GObject,
        priority: i32,
        cancellable: *mut gio::ffi::GCancellable,
        callback: gio::ffi::GAsyncReadyCallback,
        data: *mut c_void,
    );
    fn soup_session_send_finish(
        session: *mut glib::gobject_ffi::GObject,
        result: *mut gio::ffi::GAsyncResult,
        error: *mut *mut glib::ffi::GError,
    ) -> *mut gio::ffi::GInputStream;
}

type Callback = Box<dyn FnOnce(Option<String>)>;
struct Request {
    _session: glib::Object,
    message: glib::Object,
    cancel: gio::Cancellable,
    callback: Callback,
}

pub fn get(url: &str, cancel: &gio::Cancellable, callback: impl FnOnce(Option<String>) + 'static) {
    let Ok(url) = CString::new(url) else {
        callback(None);
        return;
    };
    // SAFETY: libsoup constructors return owned GObjects; Request keeps them alive through completion.
    unsafe {
        let session: glib::Object = from_glib_full(soup_session_new());
        session.set_property("timeout", 4u32);
        session.set_property("user-agent", "spot");
        let message = soup_message_new(c"GET".as_ptr(), url.as_ptr());
        if message.is_null() {
            callback(None);
            return;
        }
        let message: glib::Object = from_glib_full(message);
        // NO_REDIRECT: custom endpoints cannot redirect input to an undisclosed host.
        soup_message_set_flags(message.as_ptr(), 2);
        let request = Box::new(Request {
            _session: session.clone(),
            message: message.clone(),
            cancel: cancel.clone(),
            callback: Box::new(callback),
        });
        soup_session_send_async(
            session.as_ptr(),
            message.as_ptr(),
            0,
            cancel.to_glib_none().0,
            Some(finished),
            Box::into_raw(request).cast(),
        );
    }
}
unsafe extern "C" fn finished(
    session: *mut glib::gobject_ffi::GObject,
    result: *mut gio::ffi::GAsyncResult,
    data: *mut c_void,
) {
    // SAFETY: exactly one callback consumes the Box passed by get; finish transfers stream/error ownership.
    unsafe {
        let request = *Box::from_raw(data.cast::<Request>());
        let mut error = std::ptr::null_mut();
        let stream = soup_session_send_finish(session, result, &mut error);
        if !error.is_null() {
            glib::ffi::g_error_free(error);
        }
        if stream.is_null() {
            (request.callback)(None);
            return;
        }
        let stream: gio::InputStream = from_glib_full(stream);
        if soup_message_get_status(request.message.as_ptr()) != 200 || request.cancel.is_cancelled()
        {
            (request.callback)(None);
            return;
        }
        read(stream, request.cancel, vec![], request.callback);
    }
}
fn read(stream: gio::InputStream, cancel: gio::Cancellable, mut body: Vec<u8>, callback: Callback) {
    let next = stream.clone();
    let c = cancel.clone();
    stream.read_bytes_async(
        4096,
        glib::Priority::DEFAULT,
        Some(&cancel),
        move |result| match result {
            Ok(bytes) if bytes.is_empty() => callback(String::from_utf8(body).ok()),
            Ok(bytes) if body.len() + bytes.len() <= 65536 => {
                body.extend_from_slice(&bytes);
                read(next, c, body, callback);
            }
            _ => callback(None),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn native_http_success_limits_and_cancellation() {
        let context = glib::MainContext::new();
        context.with_thread_default(|| {
            for (body, expected) in [("hello".to_owned(),Some("hello".to_owned())),("x".repeat(65537),None)] {
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                listener.set_nonblocking(true).unwrap(); let port = listener.local_addr().unwrap().port();
                let server = std::thread::spawn(move || {
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    loop {
                        match listener.accept() {
                            Ok((mut stream,_)) => {
                                stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
                                let mut request = [0u8;4096]; let n = stream.read(&mut request).unwrap();
                                let headers = String::from_utf8_lossy(&request[..n]);
                                assert!(!headers.to_lowercase().contains("cookie:"));
                                let reply = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                                let _ = stream.write_all(reply.as_bytes()); return;
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(2)),
                            Err(e) => panic!("test server: {e}"),
                        }
                    }
                });
                let value = std::rc::Rc::new(std::cell::RefCell::new(None)); let v = value.clone();
                get(&format!("http://127.0.0.1:{port}/"),&gio::Cancellable::new(),move |s| *v.borrow_mut()=Some(s));
                let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while value.borrow().is_none() && std::time::Instant::now() < until { context.iteration(false); std::thread::sleep(std::time::Duration::from_millis(2)); }
                assert_eq!(value.take(),Some(expected)); server.join().unwrap();
            }
            let cancelled = gio::Cancellable::new(); cancelled.cancel();
            let value = std::rc::Rc::new(std::cell::RefCell::new(None)); let v = value.clone();
            get("https://example.invalid/",&cancelled,move |s| *v.borrow_mut()=Some(s));
            let until = std::time::Instant::now()+std::time::Duration::from_secs(2);
            while value.borrow().is_none() && std::time::Instant::now()<until { context.iteration(false); }
            assert_eq!(value.take(),Some(None));
        }).unwrap();
    }
}

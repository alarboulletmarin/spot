//! spot — wakes the resident instance, or starts it.
//!
//! This binary must stay GTK-free: loading GTK + libadwaita costs ~20 ms at exec
//! (the Python version paid ~200 ms for the same imports), which is the whole
//! point of having a separate launcher.

use gio::glib;
use gio::prelude::*;
use std::os::unix::process::CommandExt;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// $SPOT_APP_ID lets a development build run next to the installed one (same in spot-resident).
fn app_id() -> String {
    std::env::var("SPOT_APP_ID").unwrap_or_else(|_| "dev.andrea.Spot".into())
}

/// Ask the running instance to show its window, without loading GTK.
///
/// Importing GTK + libadwaita is what made the Python version take ~200 ms to start;
/// this only needs GIO. The activation token is forwarded so the window can take
/// focus under Wayland.
fn wake_resident_instance() -> bool {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };
    let platform_data = glib::VariantDict::new(None);
    for (key, env) in [
        ("activation-token", "XDG_ACTIVATION_TOKEN"),
        ("desktop-startup-id", "DESKTOP_STARTUP_ID"),
    ] {
        if let Some(value) = std::env::var(env).ok().filter(|v| !v.is_empty()) {
            platform_data.insert_value(key, &value.to_variant());
        }
    }
    let id = app_id();
    bus.call_sync(
        Some(&id),
        &format!("/{}", id.replace('.', "/")),
        "org.freedesktop.Application",
        "Activate",
        Some(&glib::Variant::tuple_from_iter([platform_data.end()])),
        None,
        gio::DBusCallFlags::NONE,
        2000,
        gio::Cancellable::NONE,
    )
    .is_ok()
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("spot {VERSION}");
    } else if !args.is_empty() || !wake_resident_instance() {
        // no running instance (or an explicit --daemon / --quit): hand over to the GTK application
        let resident = std::env::current_exe().map(|exe| exe.with_file_name("spot-resident"));
        let error = std::process::Command::new(resident.unwrap_or_default())
            .args(&args)
            .exec();
        eprintln!("spot: cannot start spot-resident: {error}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

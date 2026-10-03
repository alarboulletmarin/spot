//! What a result row is, and the non-application sources: system commands, shell commands, files.

use crate::search::{fuzzy_score, home};
use crate::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

pub type Action = Box<dyn Fn(&gdk::AppLaunchContext) -> Result<(), glib::Error>>;

/// One result row. `activate` launches it; `alt` is the Ctrl+Enter action.
pub struct Hit {
    pub score: i32,
    pub title: String,
    pub subtitle: String,
    pub kind: String,
    pub icon: gio::Icon,
    pub activate: Action,
    pub alt: Option<Action>,
    /// Local file results: lets duplicates across sources collapse.
    pub path: Option<String>,
}

/// Prints a failed launch the way every action reports it.
pub fn report_launch_error(error: &glib::Error) {
    eprintln!(
        "spot: {}",
        tr("Launch failed: %s").replacen("%s", error.message(), 1)
    );
}

pub fn themed(name: &str) -> gio::Icon {
    gio::ThemedIcon::new(name).upcast()
}

pub fn dbus_call(
    bus_type: gio::BusType,
    name: &'static str,
    path: &'static str,
    interface: &'static str,
    method: &'static str,
    args: Option<glib::Variant>,
) -> Result<(), glib::Error> {
    gio::bus_get_sync(bus_type, gio::Cancellable::NONE)?.call(
        Some(name),
        path,
        interface,
        method,
        args.as_ref(),
        None,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        |reply| {
            if let Err(error) = reply {
                report_launch_error(&error);
            }
        },
    );
    Ok(())
}

/// Lock, suspend, log out…: literal matches only, the labels are short.
pub fn system_results(query: &str) -> Vec<Hit> {
    use gio::BusType::{Session, System};
    const SM: (&str, &str, &str) = (
        "org.gnome.SessionManager",
        "/org/gnome/SessionManager",
        "org.gnome.SessionManager",
    );
    // label, icon, bus, (name, object path, interface), method, arguments
    let commands = [
        (
            tr("Lock screen"),
            "system-lock-screen-symbolic",
            Session,
            (
                "org.gnome.ScreenSaver",
                "/org/gnome/ScreenSaver",
                "org.gnome.ScreenSaver",
            ),
            "Lock",
            None,
        ),
        (
            tr("Suspend"),
            "system-suspend-symbolic",
            System,
            (
                "org.freedesktop.login1",
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
            ),
            "Suspend",
            Some((true,).to_variant()),
        ),
        (
            tr("Log out"),
            "system-log-out-symbolic",
            Session,
            SM,
            "Logout",
            Some((0u32,).to_variant()),
        ),
        (
            tr("Restart"),
            "system-reboot-symbolic",
            Session,
            SM,
            "Reboot",
            None,
        ),
        (
            tr("Shut down"),
            "system-shutdown-symbolic",
            Session,
            SM,
            "Shutdown",
            None,
        ),
    ];
    let q = query.to_lowercase();
    commands
        .into_iter()
        .filter(|(label, ..)| label.to_lowercase().contains(&q))
        .map(
            |(label, icon, bus, (name, path, interface), method, args)| Hit {
                score: fuzzy_score(query, &label),
                title: label,
                subtitle: String::new(),
                kind: tr("System"),
                icon: themed(icon),
                activate: Box::new(move |_| {
                    dbus_call(bus, name, path, interface, method, args.clone())
                }),
                alt: None,
                path: None,
            },
        )
        .collect()
}

/// Nothing matched: offer to run the text as a command, if its program exists.
pub fn command_result(query: &str) -> Option<Hit> {
    let program = query.split_whitespace().next()?;
    glib::find_program_in_path(program)?;
    let command = query.to_owned();
    Some(Hit {
        score: 0,
        title: tr("Run “%s”").replacen("%s", query, 1),
        subtitle: tr("Runs in the background, without a terminal"),
        kind: tr("Command"),
        icon: themed("utilities-terminal-symbolic"),
        activate: Box::new(move |ctx| run_command(&command, Some(ctx))),
        alt: None,
        path: None,
    })
}

/// Runs `command` in the background. GLib reads `%f`, `%F`, `%u`… in it as desktop-entry field
/// codes and silently drops them (`date +%F` would run as `date +`), so every `%` is doubled.
fn run_command(command: &str, ctx: Option<&gdk::AppLaunchContext>) -> Result<(), glib::Error> {
    gio::AppInfo::create_from_commandline(
        command.replace('%', "%%"),
        None::<&str>,
        gio::AppInfoCreateFlags::NONE,
    )?
    .launch(&[], ctx)
}

/// Open the containing folder in the user's file manager.
///
/// org.freedesktop.FileManager1.ShowItems would also select the file, but it goes
/// to whichever file manager owns that bus name, and without an activation token
/// Mutter stacks the new window behind the current one, so nothing seems to happen.
fn reveal_path(path: &str, ctx: &gdk::AppLaunchContext) -> Result<(), glib::Error> {
    match gio::File::for_path(path).parent() {
        Some(folder) => gio::AppInfo::launch_default_for_uri(&folder.uri(), Some(ctx)),
        None => Ok(()),
    }
}

/// A plocate or search-provider hit on a local path; None when the path is gone.
pub fn file_result(path: &str, score: i32) -> Option<Hit> {
    let is_dir = std::fs::metadata(path).ok()?.is_dir(); // None: stale index entry
    let name = std::path::Path::new(path)
        .file_name()?
        .to_string_lossy()
        .into_owned();
    let content_type = if is_dir {
        "inode/directory".into()
    } else {
        gio::content_type_guess(Some(&name), None).0
    };
    let (open, reveal) = (path.to_owned(), path.to_owned());
    Some(Hit {
        score,
        title: name,
        subtitle: path.replacen(home(), "~", 1),
        kind: if is_dir { tr("Folder") } else { tr("File") },
        icon: gio::content_type_get_icon(&content_type),
        activate: Box::new(move |ctx| {
            gio::AppInfo::launch_default_for_uri(&gio::File::for_path(&open).uri(), Some(ctx))
        }),
        alt: Some(Box::new(move |ctx| reveal_path(&reveal, ctx))),
        path: Some(path.to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command() {
        assert_eq!(command_result("ls -la").unwrap().kind, tr("Command"));
        assert!(command_result("no-such-program-xyz --flag").is_none());
        assert!(command_result("").is_none());
    }

    /// A `%` in a command reaches the program as typed, not eaten as a field code.
    #[test]
    fn command_keeps_percent_signs() {
        let dir = std::env::temp_dir().join(format!("spot-command-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (script, record) = (dir.join("record-args"), dir.join("args"));
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{0}.tmp' && mv '{0}.tmp' '{0}'\n",
                record.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();

        run_command(&format!("{} +%F 100% %u", script.display()), None).unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !record.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let received = std::fs::read_to_string(&record).expect("the command was not run");
        assert_eq!(received, "+%F\n100%\n%u\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn system() {
        let titles: Vec<_> = system_results("lock")
            .into_iter()
            .map(|h| h.title)
            .collect();
        assert_eq!(titles, [tr("Lock screen")]);
        assert!(system_results("zzz").is_empty());
    }

    #[test]
    fn file() {
        let dir = std::env::temp_dir().join(format!("spot-results-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.md");
        std::fs::write(&file, "").unwrap();
        let (file, dir_path) = (file.to_str().unwrap(), dir.to_str().unwrap());
        let (f, folder) = (
            file_result(file, 7).unwrap(),
            file_result(dir_path, 0).unwrap(),
        );
        assert_eq!(
            (f.kind.as_str(), f.title.as_str(), f.score),
            (tr("File").as_str(), "notes.md", 7)
        );
        assert_eq!(f.path.as_deref(), Some(file));
        assert_eq!(folder.kind, tr("Folder"));
        assert!(f.alt.is_some() && folder.alt.is_some());
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(file_result(file, 0).is_none()); // gone with the directory
    }
}

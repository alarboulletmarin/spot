//! Raycast script-command metadata and native interpreter execution. No bundled runtime.
use crate::{
    config,
    results::{Hit, themed},
    search::fuzzy_score,
    tr,
};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct Script {
    path: PathBuf,
    pub title: String,
    mode: String,
    icon: String,
    interpreter: Vec<String>,
    arguments: Vec<Argument>,
    cwd: PathBuf,
    confirmation: bool,
}
#[derive(Clone, Debug)]
struct Argument {
    kind: String,
    placeholder: String,
    optional: bool,
    encoded: bool,
    choices: Vec<(String, String)>,
}

impl Script {
    pub fn parse(path: &Path, source: &str) -> Option<Self> {
        let shebang = source.lines().next()?.strip_prefix("#!")?.trim();
        let interpreter = glib::shell_parse_argv(shebang)
            .ok()?
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let mut metadata = std::collections::HashMap::new();
        for line in source.lines().take(100) {
            let line = line.trim().trim_start_matches(['#', '/', ';']).trim();
            if let Some(meta) = line.strip_prefix("@raycast.")
                && let Some((key, value)) = meta.split_once(char::is_whitespace)
            {
                metadata.insert(key, value.trim());
            }
        }
        let title = metadata.get("title")?.to_string();
        let mode = metadata.get("mode").copied().unwrap_or("silent");
        if !["silent", "compact", "fullOutput"].contains(&mode) || title.is_empty() {
            return None;
        }
        let mut arguments = vec![];
        for i in 1..=3 {
            if let Some(raw) = metadata.get(format!("argument{i}").as_str()) {
                let v: serde_json::Value = serde_json::from_str(raw).ok()?;
                let kind = v.get("type")?.as_str()?;
                if !["text", "password", "dropdown"].contains(&kind) {
                    return None;
                }
                let choices = v
                    .get("data")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| {
                                Some((
                                    v.get("title")?.as_str()?.to_owned(),
                                    v.get("value")?.as_str()?.to_owned(),
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                arguments.push(Argument {
                    kind: kind.into(),
                    placeholder: v
                        .get("placeholder")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .into(),
                    optional: v.get("optional").and_then(|v| v.as_bool()).unwrap_or(false),
                    choices,
                    encoded: v
                        .get("percentEncoded")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                });
            }
        }
        Some(Self {
            path: path.into(),
            title,
            mode: mode.into(),
            icon: metadata.get("icon").copied().unwrap_or("").into(),
            interpreter,
            cwd: metadata
                .get("currentDirectoryPath")
                .map(|p| PathBuf::from(p.replace('~', crate::search::home())))
                .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_owned()),
            confirmation: metadata.get("needsConfirmation") == Some(&"true"),
            arguments,
        })
    }

    fn launch(&self, args: &[String]) -> Result<(), glib::Error> {
        let launcher = gio::SubprocessLauncher::new(if self.mode == "silent" {
            gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE
        } else {
            gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE
        });
        launcher.set_cwd(&self.cwd);
        let argv: Vec<_> = self
            .interpreter
            .iter()
            .map(std::ffi::OsStr::new)
            .chain(std::iter::once(self.path.as_os_str()))
            .chain(args.iter().map(std::ffi::OsStr::new))
            .collect();
        let proc = launcher.spawn(&argv)?;
        let title = self.title.clone();
        if self.mode == "silent" {
            proc.wait_check_async(gio::Cancellable::NONE, move |result| {
                if let Err(e) = result {
                    output(&title, &format!("{}: {e}", tr("Script failed")));
                }
            });
        } else if let Some(stream) = proc.stdout_pipe() {
            let p = proc.clone();
            read_output(
                stream,
                vec![],
                Box::new(move |text| {
                    let title = title.clone();
                    p.wait_check_async(gio::Cancellable::NONE, move |result| {
                        let text = match result {
                            Ok(()) => text,
                            Err(e) => format!("{text}\n{}: {e}", tr("Script failed")),
                        };
                        output(&title, &text);
                    });
                }),
            );
        }
        Ok(())
    }

    fn activate(self: &Rc<Self>) -> Result<(), glib::Error> {
        if self.arguments.is_empty() && !self.confirmation {
            return self.launch(&[]);
        }
        let win = gtk::Window::builder()
            .title(&self.title)
            .default_width(420)
            .modal(true)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(20);
        content.set_margin_bottom(20);
        content.set_margin_start(20);
        content.set_margin_end(20);
        if self.confirmation {
            content.append(&gtk::Label::new(Some(&tr("Run this script?"))));
        }
        let mut values: Vec<Box<dyn Fn() -> String>> = vec![];
        for arg in &self.arguments {
            content.append(&gtk::Label::new(Some(&arg.placeholder)));
            if arg.kind == "dropdown" {
                let labels: Vec<_> = arg
                    .choices
                    .iter()
                    .map(|(label, _)| label.as_str())
                    .collect();
                let dropdown = gtk::DropDown::from_strings(&labels);
                let choices = arg.choices.clone();
                let d = dropdown.clone();
                values.push(Box::new(move || {
                    choices
                        .get(d.selected() as usize)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                }));
                content.append(&dropdown);
            } else {
                let entry = gtk::Entry::builder()
                    .visibility(arg.kind != "password")
                    .build();
                let e = entry.clone();
                values.push(Box::new(move || e.text().to_string()));
                content.append(&entry);
            }
        }
        let error = gtk::Label::new(None);
        error.set_wrap(true);
        content.append(&error);
        let run = gtk::Button::with_label(&tr("Run"));
        content.append(&run);
        let script = self.clone();
        let w = win.clone();
        run.connect_clicked(move |_| {
            let args: Vec<String> = values.iter().map(|get| get()).collect();
            if args
                .iter()
                .zip(&script.arguments)
                .any(|(v, a)| v.is_empty() && !a.optional)
            {
                error.set_label(&tr("Fill in the required arguments"));
                return;
            }
            let args: Vec<_> = args
                .into_iter()
                .zip(&script.arguments)
                .map(|(value, arg)| {
                    if arg.encoded {
                        glib::uri_escape_string(&value, None::<&str>, false).to_string()
                    } else {
                        value
                    }
                })
                .collect();
            match script.launch(&args) {
                Ok(()) => w.close(),
                Err(e) => error.set_label(&e.to_string()),
            }
        });
        win.set_default_widget(Some(&run));
        win.set_child(Some(&content));
        attach_application(&win);
        win.present();
        Ok(())
    }
}

fn attach_application(win: &gtk::Window) {
    if let Some(app) =
        gio::Application::default().and_then(|a| a.downcast::<gtk::Application>().ok())
    {
        win.set_application(Some(&app));
    }
}
fn output(title: &str, text: &str) {
    let win = gtk::Window::builder()
        .title(title)
        .default_width(600)
        .default_height(240)
        .build();
    let view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    view.buffer().set_text(text);
    win.set_child(Some(&gtk::ScrolledWindow::builder().child(&view).build()));
    attach_application(&win);
    win.present();
}
fn read_output(stream: gio::InputStream, mut body: Vec<u8>, callback: Box<dyn FnOnce(String)>) {
    let next = stream.clone();
    stream.read_bytes_async(
        4096,
        glib::Priority::DEFAULT,
        gio::Cancellable::NONE,
        move |result| {
            match result {
                Ok(bytes) if !bytes.is_empty() => {
                    // Always drain the pipe to avoid deadlock; keep only the first 64 KiB.
                    let keep = bytes.len().min(65536 - body.len());
                    body.extend_from_slice(&bytes[..keep]);
                    read_output(next, body, callback);
                }
                _ => callback(String::from_utf8_lossy(&body).into_owned()),
            }
        },
    );
}

pub fn load() -> Vec<Rc<Script>> {
    let dir = config::config_dir().join("scripts");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    paths
        .into_iter()
        .filter(|p| p.is_file())
        .filter_map(|p| {
            if std::fs::metadata(&p).ok()?.len() > 1024 * 1024 {
                return None;
            }
            Script::parse(&p, &std::fs::read_to_string(&p).ok()?).map(Rc::new)
        })
        .collect()
}
pub fn hits(scripts: &[Rc<Script>], query: &str) -> Vec<Hit> {
    scripts
        .iter()
        .filter_map(|script| {
            let score = fuzzy_score(query, &script.title);
            if score < 0 {
                return None;
            }
            let icon = script
                .path
                .parent()
                .unwrap_or(Path::new("."))
                .join(&script.icon);
            let script = script.clone();
            Some(Hit {
                score,
                title: script.title.clone(),
                emoji: (!icon.is_file() && !script.icon.is_ascii()).then(|| script.icon.clone()),
                kind: tr("Script"),
                icon: if icon.is_file() {
                    gio::FileIcon::new(&gio::File::for_path(icon)).upcast()
                } else {
                    themed("utilities-terminal-symbolic")
                },
                verb: tr("Run"),
                activate: Box::new(move |_| script.activate()),
                ..Default::default()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unmodified_metadata() {
        let source = "#!/usr/bin/env python3\n# @raycast.schemaVersion 1\n# @raycast.title Example\n# @raycast.mode compact\n# @raycast.icon 🐍\n# @raycast.argument1 {\"type\":\"text\",\"placeholder\":\"Name\"}\nprint('hi')\n";
        let script = Script::parse(Path::new("example.py"), source).unwrap();
        assert_eq!(script.title, "Example");
        assert_eq!(script.interpreter, ["/usr/bin/env", "python3"]);
        assert_eq!(script.arguments[0].kind, "text");
        assert_eq!(script.icon, "🐍");
        assert!(Script::parse(Path::new("x"), "# @raycast.title Nope").is_none());
        assert!(Script::parse(Path::new("x"), &source.replace("compact", "inline")).is_none());
    }
    #[test]
    fn arguments_are_passed_without_shell_interpolation() {
        let dir = std::env::temp_dir().join(format!("spot-scripts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("command.sh");
        let source = "#!/bin/sh\n# @raycast.title Record\n# @raycast.mode silent\nprintf '%s' \"$1\" > received\n";
        std::fs::write(&path, source).unwrap();
        let script = Script::parse(&path, source).unwrap();
        // Synchronous subprocess for this test avoids needing a GTK display or main loop.
        let mut argv = script.interpreter.clone();
        argv.push(path.to_string_lossy().into_owned());
        argv.push("$(touch bad); a ' b %u".into());
        let status = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(dir.join("received")).unwrap(),
            "$(touch bad); a ' b %u"
        );
        assert!(!dir.join("bad").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

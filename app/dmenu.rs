//! Independent, nonresident stdin/stdout picker. No local sources or network requests.
use crate::{config, search, style, tr};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::{
    cell::{Cell, RefCell},
    io::{self, BufRead, Write},
    rc::Rc,
};

pub fn run(args: &[String]) -> glib::ExitCode {
    let mut prompt = tr("Select a line…");
    let mut custom = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dmenu" => {}
            "--allow-custom" => custom = true,
            "--prompt" if i + 1 < args.len() => {
                i += 1;
                prompt = args[i].clone();
            }
            other => {
                eprintln!("spot: unknown dmenu option: {other}");
                return glib::ExitCode::FAILURE;
            }
        }
        i += 1;
    }
    let lines: Vec<String> = match io::stdin().lock().lines().collect() {
        Ok(lines) => lines,
        Err(e) => {
            eprintln!("spot: stdin: {e}");
            return glib::ExitCode::FAILURE;
        }
    };
    let app = adw::Application::builder()
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let accepted = Rc::new(Cell::new(false));
    let accepted_result = accepted.clone();
    app.connect_activate(move |app| {
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Spot — dmenu")
            .default_width(600)
            .default_height(380)
            .css_classes(["spot"])
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.add_css_class("spot-card");
        let entry = gtk::Entry::builder()
            .placeholder_text(&prompt)
            .css_classes(["spot-entry"])
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Browse)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build();
        content.append(&entry);
        content.append(&scroller);
        win.set_child(Some(&content));
        style::install(&win);
        let shown = Rc::new(RefCell::new(Vec::<String>::new()));
        let candidates = lines.clone();
        let l = list.clone();
        let sh = shown.clone();
        let refresh = move |query: &str| {
            let mut matches: Vec<_> = candidates
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    let score = search::fuzzy_score(query, s);
                    (score >= 0).then_some((score, i, s))
                })
                .collect();
            matches.sort_by_key(|(score, i, _)| (-score, *i));
            l.remove_all();
            sh.borrow_mut().clear();
            for (_, _, line) in matches.into_iter().take(200) {
                sh.borrow_mut().push(line.clone());
                let label = gtk::Label::builder()
                    .label(line)
                    .xalign(0.)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build();
                l.append(&label);
            }
            l.select_row(l.row_at_index(0).as_ref());
        };
        refresh("");
        entry.connect_changed(move |e| refresh(&e.text()));
        let l = list.clone();
        let e = entry.clone();
        let sh = shown.clone();
        let a = accepted.clone();
        let w = win.clone();
        let choose: Rc<dyn Fn()> = Rc::new(move || {
            let selected = l
                .selected_row()
                .and_then(|r| sh.borrow().get(r.index() as usize).cloned())
                .or_else(|| (custom && sh.borrow().is_empty()).then(|| e.text().to_string()));
            if let Some(line) = selected {
                a.set(writeln!(io::stdout().lock(), "{line}").is_ok());
                w.close();
            }
        });
        let c = choose.clone();
        entry.connect_activate(move |_| c());
        list.connect_row_activated(move |_, _| choose());
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let w = win.clone();
        let l = list.clone();
        keys.connect_key_pressed(move |_, key, code, state| {
            match config::current().keys.command(key, code, state) {
                Some(config::Command::Back) => w.close(),
                Some(config::Command::Next | config::Command::Previous) => {
                    let step = if matches!(key, gdk::Key::Up | gdk::Key::p | gdk::Key::k) {
                        -1
                    } else {
                        1
                    };
                    let index = l.selected_row().map_or(0, |r| r.index());
                    if let Some(row) = l.row_at_index((index + step).max(0)) {
                        l.select_row(Some(&row));
                    }
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        win.add_controller(keys);
        win.present();
        entry.grab_focus();
    });
    app.run_with_args::<&str>(&[]);
    if accepted_result.get() {
        glib::ExitCode::SUCCESS
    } else {
        glib::ExitCode::FAILURE
    }
}

//! The launcher window: entry, result list, and the search that feeds it.

use crate::prefix;
use crate::providers::{self, SearchProvider};
use crate::results::{Hit, command_result, file_result, report_launch_error, system_results};
use crate::search::*;
use crate::style::{CARD_WIDTH, SHADOW};
use crate::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(90);

/// An application, with everything matching needs read once instead of on every keystroke.
struct AppEntry {
    info: gio::AppInfo,
    id: String,
    name: String,
    generic: Option<String>,
    aliases: Vec<String>,
    description: String,
    icon: gio::Icon,
}

/// State shared by the application and its window.
pub struct AppData {
    apps: RefCell<Vec<AppEntry>>,
    providers: Vec<Rc<SearchProvider>>,
    usage: RefCell<Usage>,
}

impl AppData {
    pub fn new(bus: Option<gio::DBusConnection>) -> Rc<Self> {
        let usage = Usage::new(glib::user_data_dir().join("spot").join("usage.json"));
        let data = Rc::new(AppData {
            apps: RefCell::default(),
            providers: providers::load(bus),
            usage: RefCell::new(usage),
        });
        data.load_apps();
        data
    }

    pub fn load_apps(&self) {
        *self.apps.borrow_mut() = gio::AppInfo::all()
            .into_iter()
            .filter(|a| a.should_show())
            .map(|info| {
                let desktop = info.downcast_ref::<gio_unix::DesktopAppInfo>();
                let mut aliases: Vec<String> = desktop.map_or(vec![], |d| {
                    d.keywords().iter().map(|k| k.to_string()).collect()
                });
                aliases.push(
                    info.executable()
                        .file_name()
                        .map_or(String::new(), |n| n.to_string_lossy().into_owned()),
                );
                AppEntry {
                    id: info.id().map_or(String::new(), |i| i.to_string()),
                    name: info.display_name().to_string(),
                    generic: desktop.and_then(|d| d.generic_name()).map(Into::into),
                    aliases,
                    description: info.description().map_or(String::new(), |d| d.to_string()),
                    icon: info.icon().unwrap_or_else(|| {
                        gio::ThemedIcon::new("application-x-executable").upcast()
                    }),
                    info,
                }
            })
            .collect();
    }
}

// The window lives as long as the process, so the widget <-> Rc<Ui> cycles are never a leak.
pub struct Ui {
    pub win: gtk::ApplicationWindow,
    entry: gtk::Entry,
    sep: gtk::Box,
    list: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    data: Rc<AppData>,
    results: RefCell<Vec<Rc<Hit>>>,
    sections: RefCell<HashMap<String, Vec<Rc<Hit>>>>, // per source, merged by render()
    query: RefCell<String>,
    generation: Cell<u32>,
    cancellable: RefCell<gio::Cancellable>,
    debounce: RefCell<Option<glib::SourceId>>,
    file_proc: RefCell<Option<gio::Subprocess>>,
}

impl Ui {
    pub fn new(app: &impl IsA<gtk::Application>, data: Rc<AppData>) -> Rc<Self> {
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .decorated(false)
            .resizable(false)
            .default_width(CARD_WIDTH + 2 * SHADOW)
            .css_classes(["spot"])
            .build();

        // the margin is transparent room for the card's drop shadow
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_top(SHADOW)
            .margin_bottom(SHADOW)
            .margin_start(SHADOW)
            .margin_end(SHADOW)
            .css_classes(["spot-card"])
            .build();
        win.set_child(Some(&card));

        let entry = gtk::Entry::builder()
            .placeholder_text(tr("Search applications and files…"))
            .hexpand(true)
            .css_classes(["spot-entry"])
            .build();
        let search = gtk::Box::builder().css_classes(["spot-search"]).build();
        search.append(
            &gtk::Image::builder()
                .icon_name("system-search-symbolic")
                .css_classes(["spot-search-icon"])
                .build(),
        );
        search.append(&entry);
        card.append(&search);

        let sep = gtk::Box::builder()
            .visible(false)
            .css_classes(["spot-sep"])
            .build();
        card.append(&sep);

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Browse)
            .css_classes(["spot-list"])
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .visible(false)
            .propagate_natural_height(true)
            .max_content_height(430)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build();
        card.append(&scroller);

        let ui = Rc::new(Ui {
            win,
            entry,
            sep,
            list,
            scroller,
            data,
            results: RefCell::default(),
            sections: RefCell::default(),
            query: RefCell::default(),
            generation: Cell::new(0),
            cancellable: RefCell::new(gio::Cancellable::new()),
            debounce: RefCell::default(),
            file_proc: RefCell::default(),
        });

        let u = ui.clone();
        ui.entry.connect_changed(move |_| u.on_changed());
        let u = ui.clone();
        ui.entry
            .connect_activate(move |_| u.activate_selected(false));
        let u = ui.clone();
        ui.list
            .connect_row_activated(move |_, row| u.activate_row(Some(row), false));

        let keys = gtk::EventControllerKey::new();
        let u = ui.clone();
        keys.connect_key_pressed(move |_, key, _, state| u.on_key(key, state));
        ui.win.add_controller(keys);
        let u = ui.clone();
        ui.win.connect_is_active_notify(move |w| {
            if !w.is_active() {
                u.dismiss(); // focus lost: get out of the way
            }
        });
        ui
    }

    // -- lifecycle -------------------------------------------------------

    /// Clear the query and show the window, ready for typing.
    pub fn present_fresh(&self) {
        self.entry.set_text("");
        self.show_results(vec![]);
        self.win.present();
        self.entry.grab_focus();
    }

    pub fn dismiss(&self) {
        self.win.set_visible(false);
    }

    // -- keyboard --------------------------------------------------------

    fn on_key(&self, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        match key {
            gdk::Key::Escape => self.dismiss(),
            gdk::Key::Return | gdk::Key::KP_Enter
                if state.contains(gdk::ModifierType::CONTROL_MASK) =>
            {
                self.activate_selected(true)
            }
            gdk::Key::Down => self.move_selection(1),
            gdk::Key::Up => self.move_selection(-1),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// Move the selection while keeping keyboard focus in the entry.
    fn move_selection(&self, delta: i32) {
        let count = self.results.borrow().len() as i32;
        if count == 0 {
            return;
        }
        let current = self.list.selected_row().map_or(-1, |r| r.index());
        let index = (current + delta).clamp(0, count - 1);
        let Some(row) = self.list.row_at_index(index) else {
            return;
        };
        self.list.select_row(Some(&row));
        if let Some(bounds) = row.compute_bounds(&self.list) {
            self.scroller
                .vadjustment()
                .clamp_page(bounds.y() as f64, (bounds.y() + bounds.height()) as f64);
        }
    }

    // -- search ----------------------------------------------------------

    fn on_changed(self: &Rc<Self>) {
        if let Some(id) = self.debounce.take() {
            id.remove();
        }
        let ui = self.clone();
        *self.debounce.borrow_mut() = Some(glib::timeout_add_local(DEBOUNCE, move || {
            ui.search();
            glib::ControlFlow::Break
        }));
    }

    fn search(self: &Rc<Self>) {
        self.debounce.take(); // fired: its id is gone, removing it later would be an error
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        // a newer keystroke supersedes everything in flight
        self.cancellable.replace(gio::Cancellable::new()).cancel();
        if let Some(proc) = self.file_proc.take() {
            proc.force_exit();
        }
        let query = self.entry.text().trim().to_owned();
        self.query.replace(query.clone());

        self.sections.borrow_mut().clear();
        if query.is_empty() {
            return self.render();
        }
        if let Some(hits) = prefix::hits(&query) {
            // `!ls`, `search: …`: the query is an instruction, not something to look for
            let hits = hits.into_iter().map(Rc::new).collect();
            self.sections.borrow_mut().insert("prefix".into(), hits);
            return self.render();
        }
        let apps: Vec<_> = self
            .search_apps(&query)
            .into_iter()
            .take(MAX_APPS)
            .map(Rc::new)
            .collect();
        self.sections.borrow_mut().insert("apps".into(), apps);
        self.render(); // show right away, D-Bus and plocate answers follow

        if query.chars().count() >= MIN_PROVIDER_QUERY {
            let ui = self.clone();
            let callback: providers::Callback = Rc::new(move |id, hits| {
                if ui.generation.get() == generation {
                    ui.sections
                        .borrow_mut()
                        .insert(id.to_owned(), hits.into_iter().map(Rc::new).collect());
                    ui.render();
                }
            });
            let cancellable = self.cancellable.borrow().clone();
            for provider in &self.data.providers {
                let terms = query.split_whitespace().map(String::from).collect();
                provider.search(terms, &cancellable, callback.clone());
            }
        }
        if query.chars().count() >= MIN_FILE_QUERY {
            self.search_files(query, generation);
        }
    }

    fn render(&self) {
        let sections = self.sections.borrow();
        let order = ["prefix", "apps"]
            .into_iter()
            .chain(self.data.providers.iter().map(|p| p.desktop_id.as_str()))
            .chain(["files"]);
        let mut results: Vec<Rc<Hit>> = vec![];
        let mut seen = HashSet::new();
        for hit in order.filter_map(|k| sections.get(k)).flatten() {
            if let Some(path) = &hit.path
                && !seen.insert(path.as_str())
            {
                continue; // the same file found by a provider and by plocate
            }
            results.push(hit.clone());
        }
        if results.is_empty() && !sections.contains_key("prefix") {
            results.extend(command_result(&self.query.borrow()).map(Rc::new));
        }
        drop(sections);
        self.show_results(results);
    }

    fn search_apps(&self, query: &str) -> Vec<Hit> {
        let usage = self.data.usage.borrow();
        let mut hits: Vec<Hit> = self
            .data
            .apps
            .borrow()
            .iter()
            .filter_map(|app| {
                let score = app_score(query, &app.name, app.generic.as_deref(), &app.aliases);
                (score >= 0).then(|| {
                    let (info, id, data) = (app.info.clone(), app.id.clone(), self.data.clone());
                    Hit {
                        score: score + usage.bonus(&app.id),
                        title: app.name.clone(),
                        subtitle: app.description.clone(),
                        kind: tr("Application"),
                        icon: app.icon.clone(),
                        activate: Box::new(move |ctx| {
                            info.launch(&[], Some(ctx))?;
                            if let Err(error) = data.usage.borrow_mut().bump(&id) {
                                eprintln!("spot: cannot save usage counts: {error}");
                            }
                            Ok(())
                        }),
                        alt: None,
                        path: None,
                    }
                })
            })
            .collect();
        hits.extend(system_results(query));
        hits.sort_by_key(|h| -h.score);
        hits
    }

    fn search_files(self: &Rc<Self>, query: String, generation: u32) {
        // --basename: match the file name only; the whole path would surface
        // every file under a matching directory. No --limit: plocate lists
        // paths in sorted order, so a limit would be spent on /etc and ~/.cache
        // before reaching anything we want to show.
        let proc = gio::Subprocess::newv(
            &["plocate", "--ignore-case", "--basename", "--", &query].map(std::ffi::OsStr::new),
            gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
        );
        let Ok(proc) = proc else { return }; // plocate not installed: applications only
        *self.file_proc.borrow_mut() = Some(proc.clone());
        let ui = self.clone();
        proc.communicate_utf8_async(None::<String>, gio::Cancellable::NONE, move |result| {
            if ui.generation.get() != generation {
                return; // superseded by a newer keystroke
            }
            ui.file_proc.take();
            if let Ok((Some(stdout), _)) = result {
                ui.on_files(&stdout, &query);
            }
        });
    }

    fn on_files(&self, stdout: &str, query: &str) {
        let mut candidates: Vec<(i32, usize, &str)> = stdout
            .lines()
            .filter(|p| is_wanted_path(p))
            .map(|p| {
                let name = p.rsplit('/').next().unwrap_or(p);
                (fuzzy_score(query, name), p.chars().count(), p)
            })
            .collect();
        candidates.sort_by_key(|&(score, len, _)| (-score, len));
        let files: Vec<_> = candidates
            .into_iter()
            .filter_map(|(score, _, path)| file_result(path, score))
            .take(MAX_FILES)
            .map(Rc::new)
            .collect();
        if !files.is_empty() {
            self.sections.borrow_mut().insert("files".into(), files);
            self.render();
        }
    }

    // -- display ---------------------------------------------------------

    fn show_results(&self, results: Vec<Rc<Hit>>) {
        // keep the user's pick when late results land
        let index = self.list.selected_row().map_or(0, |r| r.index());
        self.list.remove_all();
        for hit in &results {
            self.list.append(&row_for(hit));
        }
        let visible = !results.is_empty();
        let last = results.len() as i32 - 1;
        *self.results.borrow_mut() = results;
        self.scroller.set_visible(visible);
        self.sep.set_visible(visible);
        if visible {
            self.list
                .select_row(self.list.row_at_index(index.min(last)).as_ref());
        }
    }

    // -- launching -------------------------------------------------------

    fn activate_selected(&self, alt: bool) {
        self.activate_row(self.list.selected_row().as_ref(), alt);
    }

    fn activate_row(&self, row: Option<&gtk::ListBoxRow>, alt: bool) {
        let Some(hit) = row.and_then(|r| self.results.borrow().get(r.index() as usize).cloned())
        else {
            return;
        };
        let Some(action) = (if alt {
            hit.alt.as_ref()
        } else {
            Some(&hit.activate)
        }) else {
            return;
        };
        if let Err(error) = action(&WidgetExt::display(&self.win).app_launch_context()) {
            report_launch_error(&error);
        }
        self.dismiss();
    }
}

fn label(text: &str, css: Option<&str>) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    if let Some(css) = css {
        label.add_css_class(css);
    }
    label
}

fn row_for(hit: &Hit) -> gtk::ListBoxRow {
    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let icon = gtk::Image::from_gicon(&hit.icon);
    icon.add_css_class("spot-icon");
    row_box.append(&icon);

    let texts = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    texts.append(&label(&hit.title, Some("spot-title")));
    if !hit.subtitle.is_empty() {
        texts.append(&label(&hit.subtitle, Some("spot-sub")));
    }
    row_box.append(&texts);

    let kind = gtk::Label::new(Some(&hit.kind));
    kind.add_css_class("spot-kind");
    kind.set_valign(gtk::Align::Center);
    row_box.append(&kind);

    gtk::ListBoxRow::builder()
        .css_classes(["spot-row"])
        .child(&row_box)
        .build()
}

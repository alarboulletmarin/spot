//! How the window looks: the built-in styles, the colour-scheme choice, and the user's own CSS.
//!
//! Colours come from libadwaita's named colours (`@window_bg_color`, `@accent_bg_color`…), so
//! every style follows the desktop: light or dark, and the accent colour where libadwaita
//! publishes it (1.6+). Settings live in `~/.config/spot/spot.conf` and are re-read as soon as
//! the file changes, so a style can be tried without restarting the resident process.

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

/// Width of the card; the window adds `SHADOW` on every side so the drop shadow is not clipped.
pub const CARD_WIDTH: i32 = 720;
pub const SHADOW: i32 = 36;

pub const CONFIG_FILE: &str = "spot.conf";
const USER_CSS_FILE: &str = "style.css";
const RELOAD_DELAY: Duration = Duration::from_millis(120);

/// Structure and the default style. `glass` and `compact` only override what differs.
const CSS: &str = "
window.spot { background: transparent; }

.spot-card {
    background-color: @window_bg_color;
    color: @window_fg_color;
    border-radius: 18px;
    box-shadow: 0 0 0 1px alpha(currentColor, 0.10),
                0 8px 24px alpha(black, 0.28),
                0 2px 6px alpha(black, 0.16);
}

.spot-search { padding: 0 22px; }
.spot-search-icon { -gtk-icon-size: 22px; opacity: 0.5; }
.spot-entry {
    font-size: 1.4rem;
    padding: 20px 14px;
    min-height: 0;
    background: none;
    color: inherit;
    border: none;
    box-shadow: none;
    outline: none;
    caret-color: @accent_color;
}
.spot-entry:focus-within { outline: none; box-shadow: none; }
.spot-entry placeholder { opacity: 0.4; }

.spot-sep { background-color: alpha(currentColor, 0.10); min-height: 1px; }

.spot-list { background: none; color: inherit; margin: 8px 10px 10px 10px; }
.spot-row {
    padding: 7px 12px;
    margin: 1px 0;
    border-radius: 12px;
    transition: background-color 80ms ease-out;
}
.spot-row:hover { background-color: alpha(currentColor, 0.06); }
.spot-row:selected { background-color: @accent_bg_color; color: @accent_fg_color; }
.spot-icon { -gtk-icon-size: 32px; }
.spot-title { font-size: 1rem; font-weight: 500; }
.spot-sub { font-size: 0.82rem; opacity: 0.62; }
.spot-kind {
    font-size: 0.72rem;
    font-weight: 600;
    padding: 2px 9px;
    border-radius: 99px;
    background-color: alpha(currentColor, 0.08);
    opacity: 0.75;
}
.spot-row:selected .spot-sub { opacity: 0.85; }
.spot-row:selected .spot-kind { background-color: alpha(@accent_fg_color, 0.18); opacity: 1; }

/* glass: translucent, rounder, selection as a soft tint of the accent colour */
window.spot.glass .spot-card {
    background-color: alpha(@window_bg_color, 0.80);
    border-radius: 26px;
    box-shadow: 0 0 0 1px alpha(currentColor, 0.12),
                inset 0 1px 0 alpha(white, 0.10),
                0 10px 26px alpha(black, 0.30);
}
window.spot.glass .spot-entry { font-weight: 300; padding: 24px 14px; }
window.spot.glass .spot-sep { background-color: alpha(currentColor, 0.07); }
window.spot.glass .spot-row { border-radius: 14px; }
window.spot.glass .spot-row:selected { background-color: alpha(@accent_bg_color, 0.28); color: inherit; }
window.spot.glass .spot-row:selected .spot-sub { opacity: 0.62; }
window.spot.glass .spot-row:selected .spot-kind { background-color: alpha(@accent_bg_color, 0.30); opacity: 0.9; }
window.spot.glass .spot-kind { background-color: alpha(currentColor, 0.07); }

/* compact: denser rows, small icons, a neutral tint instead of the accent colour for the selection */
window.spot.compact .spot-card { border-radius: 12px; }
window.spot.compact .spot-search { padding: 0 14px; }
window.spot.compact .spot-search-icon { -gtk-icon-size: 18px; }
window.spot.compact .spot-entry { font-size: 1.1rem; padding: 12px 10px; }
window.spot.compact .spot-list { margin: 4px 6px 6px 6px; }
window.spot.compact .spot-row { padding: 3px 10px; margin: 0; border-radius: 8px; }
window.spot.compact .spot-icon { -gtk-icon-size: 24px; }
window.spot.compact .spot-title { font-size: 0.95rem; font-weight: 400; }
window.spot.compact .spot-sub { font-size: 0.76rem; }
window.spot.compact .spot-row:selected { background-color: alpha(currentColor, 0.12); color: inherit; }
window.spot.compact .spot-row:selected .spot-sub { opacity: 0.62; }
window.spot.compact .spot-row:selected .spot-kind { background-color: alpha(currentColor, 0.10); opacity: 0.75; }
";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Style {
    #[default]
    Default,
    Glass,
    Compact,
}

impl Style {
    const ALL: [(&'static str, Style); 3] = [
        ("default", Style::Default),
        ("glass", Style::Glass),
        ("compact", Style::Compact),
    ];

    fn parse(name: &str) -> Option<Self> {
        let name = name.trim().to_lowercase();
        Self::ALL.iter().find(|(n, _)| *n == name).map(|&(_, s)| s)
    }

    /// The default style is the absence of a class: the other styles only override it.
    fn class(self) -> Option<&'static str> {
        match self {
            Style::Default => None,
            Style::Glass => Some("glass"),
            Style::Compact => Some("compact"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Scheme {
    #[default]
    System,
    Light,
    Dark,
}

impl Scheme {
    fn parse(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "system" | "default" => Some(Scheme::System),
            "light" => Some(Scheme::Light),
            "dark" => Some(Scheme::Dark),
            _ => None,
        }
    }

    fn to_adw(self) -> adw::ColorScheme {
        match self {
            Scheme::System => adw::ColorScheme::Default,
            Scheme::Light => adw::ColorScheme::ForceLight,
            Scheme::Dark => adw::ColorScheme::ForceDark,
        }
    }
}

/// What `spot.conf` asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Appearance {
    pub style: Style,
    pub scheme: Scheme,
}

impl Appearance {
    /// Parses the file's content. A missing key keeps its default; an invalid value is
    /// reported on stderr and also keeps its default, so a typo never breaks the launcher.
    fn parse(data: &str) -> Self {
        let mut appearance = Appearance::default();
        let keyfile = glib::KeyFile::new();
        if let Err(error) = keyfile.load_from_data(data, glib::KeyFileFlags::NONE) {
            eprintln!("spot: cannot read {CONFIG_FILE}: {}", error.message());
            return appearance;
        }
        if let Ok(value) = keyfile.string("Appearance", "Style") {
            match Style::parse(&value) {
                Some(style) => appearance.style = style,
                None => eprintln!(
                    "spot: unknown Style “{value}” in {CONFIG_FILE} (default, glass, compact)"
                ),
            }
        }
        if let Ok(value) = keyfile.string("Appearance", "ColorScheme") {
            match Scheme::parse(&value) {
                Some(scheme) => appearance.scheme = scheme,
                None => eprintln!(
                    "spot: unknown ColorScheme “{value}” in {CONFIG_FILE} (system, light, dark)"
                ),
            }
        }
        appearance
    }
}

pub fn config_dir() -> PathBuf {
    glib::user_config_dir().join("spot")
}

/// Owns the CSS providers and keeps the window in line with the files on disk.
struct Theme {
    win: gtk::ApplicationWindow,
    user_css: gtk::CssProvider,
    dir: PathBuf,
    pending: RefCell<Option<glib::SourceId>>,
}

impl Theme {
    fn apply(&self) {
        let appearance = std::fs::read_to_string(self.dir.join(CONFIG_FILE))
            .map_or_else(|_| Appearance::default(), |data| Appearance::parse(&data));
        adw::StyleManager::default().set_color_scheme(appearance.scheme.to_adw());
        for (_, style) in Style::ALL {
            if let Some(class) = style.class() {
                self.win.remove_css_class(class);
            }
        }
        if let Some(class) = appearance.style.class() {
            self.win.add_css_class(class);
        }
        // a missing file is the normal case: an empty sheet drops whatever was loaded before
        let css = std::fs::read_to_string(self.dir.join(USER_CSS_FILE)).unwrap_or_default();
        self.user_css.load_from_string(&css);
    }

    /// Editors save through a temporary file and a rename, which is several events: wait
    /// for them to settle and apply once.
    fn schedule_apply(self: &Rc<Self>) {
        if let Some(id) = self.pending.take() {
            id.remove();
        }
        let theme = self.clone();
        *self.pending.borrow_mut() = Some(glib::timeout_add_local(RELOAD_DELAY, move || {
            theme.pending.take(); // fired: its id is gone
            theme.apply();
            glib::ControlFlow::Break
        }));
    }
}

/// Load the style sheets, apply the user's settings, and watch them for changes.
///
/// Call once, before the window is first shown.
pub fn install(win: &gtk::ApplicationWindow) {
    let display = gdk::Display::default().expect("no display");

    let base = gtk::CssProvider::new();
    base.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &display,
        &base,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // above the application's own rules, so `style.css` can override any of them
    let user_css = gtk::CssProvider::new();
    user_css.connect_parsing_error(|_, section, error| {
        eprintln!(
            "spot: {USER_CSS_FILE}:{}: {}",
            section.start_location().lines() + 1,
            error.message()
        );
    });
    gtk::style_context_add_provider_for_display(
        &display,
        &user_css,
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );

    let theme = Rc::new(Theme {
        win: win.clone(),
        user_css,
        dir: config_dir(),
        pending: RefCell::default(),
    });
    theme.apply();
    watch(&theme);
}

/// Reload when `spot.conf` or `style.css` changes. The directory is watched, not the files,
/// so creating them later (or replacing them on save) is seen too.
fn watch(theme: &Rc<Theme>) {
    // the monitor needs the directory to exist; an empty one is harmless
    let _ = std::fs::create_dir_all(&theme.dir);
    let monitor = gio::File::for_path(&theme.dir)
        .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE);
    let Ok(monitor) = monitor else { return };
    let t = theme.clone();
    monitor.connect_changed(move |_, file, other, _| {
        let is_ours = |f: &gio::File| f.basename().is_some_and(|n| is_config_name(n.as_path()));
        if is_ours(file) || other.is_some_and(is_ours) {
            t.schedule_apply();
        }
    });
    std::mem::forget(monitor); // the window lives as long as the process, and so does the watch
}

fn is_config_name(name: &Path) -> bool {
    name == Path::new(CONFIG_FILE) || name == Path::new(USER_CSS_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_keeps_the_defaults() {
        assert_eq!(Appearance::parse(""), Appearance::default());
        assert_eq!(Appearance::default().style, Style::Default);
        assert_eq!(Appearance::default().scheme, Scheme::System);
    }

    #[test]
    fn reads_style_and_scheme() {
        let a = Appearance::parse("[Appearance]\nStyle=glass\nColorScheme=dark\n");
        assert_eq!((a.style, a.scheme), (Style::Glass, Scheme::Dark));
        let a = Appearance::parse("[Appearance]\nStyle = Compact \nColorScheme=LIGHT\n");
        assert_eq!((a.style, a.scheme), (Style::Compact, Scheme::Light));
    }

    #[test]
    fn a_typo_keeps_the_default_for_that_key_only() {
        let a = Appearance::parse("[Appearance]\nStyle=glas\nColorScheme=dark\n");
        assert_eq!((a.style, a.scheme), (Style::Default, Scheme::Dark));
    }

    #[test]
    fn broken_file_does_not_panic() {
        assert_eq!(Appearance::parse("not a keyfile"), Appearance::default());
    }

    #[test]
    fn every_style_has_a_rule_in_the_sheet() {
        for (name, style) in Style::ALL {
            assert_eq!(Style::parse(name), Some(style));
            if let Some(class) = style.class() {
                assert!(CSS.contains(&format!("window.spot.{class} ")), "{class}");
            }
        }
    }

    #[test]
    fn only_our_two_files_trigger_a_reload() {
        assert!(is_config_name(Path::new("spot.conf")));
        assert!(is_config_name(Path::new("style.css")));
        assert!(!is_config_name(Path::new("spot.conf~")));
        assert!(!is_config_name(Path::new("other")));
    }

    /// The example in the README is what users copy: it must parse without a complaint.
    #[test]
    fn readme_example_parses() {
        let readme = include_str!("../README.md");
        let block = readme
            .split("```ini\n")
            .skip(1) // what comes before the first example is prose, which may mention the group
            .filter_map(|rest| rest.split("```").next())
            .find(|block| block.contains("[Appearance]"))
            .expect("an [Appearance] example in the README");
        let a = Appearance::parse(block);
        assert_eq!((a.style, a.scheme), (Style::Glass, Scheme::System));
    }
}

//! How the window looks: the built-in styles, the colour-scheme choice, and the user's own CSS.
//!
//! Colours come from libadwaita's named colours (`@window_bg_color`, `@accent_bg_color`…), so
//! every style follows the desktop: light or dark, and the accent colour where libadwaita
//! publishes it (1.6+). A palette redefines those colours. Settings live in
//! `~/.config/spot/spot.conf` (see `config`) and are re-read as soon as the file changes, so a
//! style can be tried without restarting the resident process.

use crate::config::{self, CONFIG_FILE};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

/// Width of the card; the window adds `SHADOW` on every side so the drop shadow is not clipped.
pub const CARD_WIDTH: i32 = 720;
pub const SHADOW: i32 = 36;

const USER_CSS_FILE: &str = "style.css";
const THEMES_DIR: &str = "themes";
/// The value of `Palette` that asks for the desktop's own colours.
pub const NO_PALETTE: &str = "none";
const RELOAD_DELAY: Duration = Duration::from_millis(120);

/// Structure and the default style. The other styles only override what differs.
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
/* the row's number, shown while the modifier that picks it is held */
.spot-pick { font-size: 0.72rem; font-weight: 600; min-width: 1em; opacity: 0; }
window.spot.picking .spot-pick { opacity: 0.6; }

.spot-footer {
    padding: 7px 22px 9px 22px;
    border-top: 1px solid alpha(currentColor, 0.10);
    font-size: 0.78rem;
}
.spot-footer label { opacity: 0.55; }

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
window.spot.compact .spot-footer { padding: 4px 14px 5px 14px; }

/* flat: square, no shadow, rows from edge to edge */
window.spot.flat .spot-card { border-radius: 0; box-shadow: 0 0 0 1px alpha(currentColor, 0.18); }
window.spot.flat .spot-list { margin: 0; }
window.spot.flat .spot-row { padding: 8px 22px; margin: 0; border-radius: 0; }
window.spot.flat .spot-kind { border-radius: 3px; }

/* rounded: a pill for a card, pills for rows */
window.spot.rounded .spot-card { border-radius: 34px; }
window.spot.rounded .spot-search { padding: 0 30px; }
window.spot.rounded .spot-list { margin: 8px 14px 12px 14px; }
window.spot.rounded .spot-row { padding: 7px 18px; border-radius: 99px; }
window.spot.rounded .spot-footer { padding: 7px 32px 13px 32px; }

/* mono: a terminal's look, fixed-width type and the selection in reverse video */
window.spot.mono .spot-card { border-radius: 6px; font-family: monospace; }
window.spot.mono .spot-entry { font-family: monospace; font-size: 1.2rem; padding: 16px 12px; }
window.spot.mono .spot-list { margin: 6px; }
window.spot.mono .spot-row { padding: 5px 12px; margin: 0; border-radius: 3px; }
window.spot.mono .spot-icon { -gtk-icon-size: 24px; }
window.spot.mono .spot-kind { border-radius: 3px; background: none; box-shadow: inset 0 0 0 1px alpha(currentColor, 0.30); }
window.spot.mono .spot-row:selected { background-color: @window_fg_color; color: @window_bg_color; }
window.spot.mono .spot-row:selected .spot-kind { background: none; opacity: 1; }
";

/// The built-in palettes: name, then background, text, accent and text on the accent, for the
/// light variant and for the dark one.
const PALETTES: [(&str, [&str; 4], [&str; 4]); 12] = [
    (
        "catppuccin",
        ["#eff1f5", "#4c4f69", "#8839ef", "#eff1f5"],
        ["#1e1e2e", "#cdd6f4", "#cba6f7", "#1e1e2e"],
    ),
    (
        "gruvbox",
        ["#fbf1c7", "#3c3836", "#af3a03", "#fbf1c7"],
        ["#282828", "#ebdbb2", "#fe8019", "#282828"],
    ),
    (
        "nord",
        ["#eceff4", "#2e3440", "#5e81ac", "#eceff4"],
        ["#2e3440", "#eceff4", "#88c0d0", "#2e3440"],
    ),
    (
        "solarized",
        ["#fdf6e3", "#586e75", "#268bd2", "#fdf6e3"],
        ["#002b36", "#93a1a1", "#268bd2", "#fdf6e3"],
    ),
    (
        "dracula",
        ["#fffbeb", "#1f1f1f", "#644ac9", "#fffbeb"],
        ["#282a36", "#f8f8f2", "#bd93f9", "#282a36"],
    ),
    (
        "tokyo-night",
        ["#e1e2e7", "#3760bf", "#2e7de9", "#e1e2e7"],
        ["#1a1b26", "#c0caf5", "#7aa2f7", "#1a1b26"],
    ),
    (
        "rose-pine",
        ["#faf4ed", "#575279", "#907aa9", "#faf4ed"],
        ["#191724", "#e0def4", "#c4a7e7", "#191724"],
    ),
    (
        "everforest",
        ["#fdf6e3", "#5c6a72", "#5f8700", "#fdf6e3"],
        ["#2d353b", "#d3c6aa", "#a7c080", "#2d353b"],
    ),
    (
        "one",
        ["#fafafa", "#383a42", "#4078f2", "#fafafa"],
        ["#282c34", "#abb2bf", "#61afef", "#282c34"],
    ),
    (
        "kanagawa",
        ["#f2ecbc", "#545464", "#4d699b", "#f2ecbc"],
        ["#1f1f28", "#dcd7ba", "#7e9cd8", "#1f1f28"],
    ),
    (
        "ayu",
        ["#fcfcfc", "#5c6166", "#f2590c", "#fcfcfc"],
        ["#0d1017", "#bfbdb6", "#e6b450", "#0d1017"],
    ),
    (
        "github",
        ["#ffffff", "#1f2328", "#0969da", "#ffffff"],
        ["#0d1117", "#e6edf3", "#1f6feb", "#ffffff"],
    ),
];

/// The names `Palette` can take: the built-in ones, then the user's `themes/<name>.css` in
/// `dir`. `<name>-dark.css` is the dark variant of `<name>`, not a palette of its own.
pub fn palettes(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = PALETTES.iter().map(|(name, ..)| name.to_string()).collect();
    let mut user: Vec<String> = std::fs::read_dir(dir.join(THEMES_DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| Some(entry.file_name().to_str()?.strip_suffix(".css")?.to_owned()))
        .filter(|name| !name.ends_with("-dark") && !names.contains(name))
        .collect();
    user.sort();
    names.extend(user);
    names
}

/// The sheet of a palette: the user's file when there is one, so that a built-in palette can
/// be replaced, else the built-in colours. Empty for a name that is neither.
fn palette_css(dir: &Path, name: &str, dark: bool) -> String {
    let read = |file: String| std::fs::read_to_string(dir.join(THEMES_DIR).join(file)).ok();
    let user = dark
        .then(|| read(format!("{name}-dark.css")))
        .flatten()
        .or_else(|| read(format!("{name}.css")));
    let built_in = || {
        let (_, light, dark_colours) = PALETTES.iter().find(|(known, ..)| *known == name)?;
        let [bg, fg, accent, on_accent] = if dark { dark_colours } else { light };
        Some(format!(
            "@define-color window_bg_color {bg};\n@define-color window_fg_color {fg};\n\
             @define-color accent_bg_color {accent};\n@define-color accent_color {accent};\n\
             @define-color accent_fg_color {on_accent};\n"
        ))
    };
    user.or_else(built_in).unwrap_or_default()
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Style {
    #[default]
    Default,
    Glass,
    Compact,
    Flat,
    Rounded,
    Mono,
}

impl Style {
    pub const ALL: [(&'static str, Style); 6] = [
        ("default", Style::Default),
        ("glass", Style::Glass),
        ("compact", Style::Compact),
        ("flat", Style::Flat),
        ("rounded", Style::Rounded),
        ("mono", Style::Mono),
    ];

    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim().to_lowercase();
        Self::ALL.iter().find(|(n, _)| *n == name).map(|&(_, s)| s)
    }

    /// The default style is the absence of a class: the other styles only override it.
    fn class(self) -> Option<&'static str> {
        match self {
            Style::Default => None,
            Style::Glass => Some("glass"),
            Style::Compact => Some("compact"),
            Style::Flat => Some("flat"),
            Style::Rounded => Some("rounded"),
            Style::Mono => Some("mono"),
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
    pub const ALL: [(&'static str, Scheme); 3] = [
        ("system", Scheme::System),
        ("light", Scheme::Light),
        ("dark", Scheme::Dark),
    ];

    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim().to_lowercase();
        let name = if name == "default" { "system" } else { &name };
        Self::ALL.iter().find(|(n, _)| *n == name).map(|&(_, s)| s)
    }

    fn to_adw(self) -> adw::ColorScheme {
        match self {
            Scheme::System => adw::ColorScheme::Default,
            Scheme::Light => adw::ColorScheme::ForceLight,
            Scheme::Dark => adw::ColorScheme::ForceDark,
        }
    }
}

/// How the window looks: what `spot.conf` asks for, or what the palette is previewing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Appearance {
    pub style: Style,
    pub scheme: Scheme,
    /// One of `palettes()`; `None` keeps the desktop's colours.
    pub palette: Option<String>,
}

/// Owns the CSS providers and keeps the window in line with the files on disk.
struct Theme {
    win: gtk::ApplicationWindow,
    palette_css: gtk::CssProvider,
    user_css: gtk::CssProvider,
    dir: PathBuf,
    palette: RefCell<Option<String>>, // the one on screen, to load again when dark changes
    pending: RefCell<Option<glib::SourceId>>,
}

impl Theme {
    fn show(&self, appearance: &Appearance) {
        // before the scheme: changing it can flip `dark`, which loads the palette
        self.palette.replace(appearance.palette.clone());
        adw::StyleManager::default().set_color_scheme(appearance.scheme.to_adw());
        for (_, style) in Style::ALL {
            if let Some(class) = style.class() {
                self.win.remove_css_class(class);
            }
        }
        if let Some(class) = appearance.style.class() {
            self.win.add_css_class(class);
        }
        self.load_palette();
        // a missing file is the normal case: an empty sheet drops whatever was loaded before
        let css = std::fs::read_to_string(self.dir.join(USER_CSS_FILE)).unwrap_or_default();
        self.user_css.load_from_string(&css);
    }

    fn load_palette(&self) {
        let dark = adw::StyleManager::default().is_dark();
        let css = self
            .palette
            .borrow()
            .as_deref()
            .map_or(String::new(), |name| palette_css(&self.dir, name, dark));
        self.palette_css.load_from_string(&css);
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
            refresh();
            glib::ControlFlow::Break
        }));
    }
}

thread_local! {
    static THEME: OnceCell<Rc<Theme>> = const { OnceCell::new() };
}

/// Shows `appearance` without saving it: the palette does, for the row under the selection.
pub fn preview(appearance: &Appearance) {
    THEME.with(|theme| {
        if let Some(theme) = theme.get() {
            theme.show(appearance);
        }
    });
}

/// Back to what `spot.conf` says, after a preview.
pub fn revert() {
    preview(&config::current().appearance);
}

/// Reads `spot.conf` again and applies it.
pub fn refresh() {
    preview(&config::reload().appearance);
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

    // above the application's own rules, so a palette and `style.css` can override any of them
    let palette_css = gtk::CssProvider::new();
    let user_css = gtk::CssProvider::new();
    for (provider, file) in [(&palette_css, "palette"), (&user_css, USER_CSS_FILE)] {
        provider.connect_parsing_error(move |_, section, error| {
            eprintln!(
                "spot: {file}:{}: {}",
                section.start_location().lines() + 1,
                error.message()
            );
        });
    }
    gtk::style_context_add_provider_for_display(
        &display,
        &palette_css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &user_css,
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );

    let theme = Rc::new(Theme {
        win: win.clone(),
        palette_css,
        user_css,
        dir: config::config_dir(),
        palette: RefCell::default(),
        pending: RefCell::default(),
    });
    // a palette has a light and a dark variant: follow the desktop when it switches
    let t = theme.clone();
    adw::StyleManager::default().connect_dark_notify(move |_| t.load_palette());
    watch(&theme);
    THEME.with(|cell| cell.set(theme).ok().expect("style::install called twice"));
    refresh();
}

/// Reload when `spot.conf` or `style.css` changes. The directory is watched, not the files,
/// so creating them later (or replacing them on save) is seen too.
// ponytail: `themes/` is not watched, so an edited palette file shows at the next change of
// spot.conf or of the selection in the palette. Add a second monitor if palettes get edited live.
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

    #[test]
    fn names_parse_back() {
        for (name, scheme) in Scheme::ALL {
            assert_eq!(Scheme::parse(name), Some(scheme));
        }
        assert_eq!(Scheme::parse(" Default "), Some(Scheme::System));
        assert_eq!(Scheme::parse("black"), None);
    }

    #[test]
    fn built_in_palettes_define_the_colours_the_sheet_uses() {
        let nowhere = Path::new("");
        for (name, ..) in PALETTES {
            let (light, dark) = (
                palette_css(nowhere, name, false),
                palette_css(nowhere, name, true),
            );
            assert_ne!(light, dark, "{name}");
            for colour in CSS
                .split('@')
                .skip(1)
                .filter_map(|rest| rest.split_once("_color"))
            {
                let define = format!("@define-color {}_color #", colour.0);
                assert!(
                    light.contains(&define) && dark.contains(&define),
                    "{name}: {define}"
                );
            }
        }
        assert_eq!(palette_css(nowhere, "nope", true), "");
    }

    #[test]
    fn user_palettes_come_from_the_themes_directory() {
        let dir = std::env::temp_dir().join(format!("spot-themes-{}", std::process::id()));
        let themes = dir.join(THEMES_DIR);
        std::fs::create_dir_all(&themes).unwrap();
        for (file, css) in [
            ("mine.css", "light"),
            ("mine-dark.css", "dark"),
            ("plain.css", "both"),
            ("nord.css", "my nord"),
            ("notes.txt", ""),
        ] {
            std::fs::write(themes.join(file), css).unwrap();
        }
        let names = palettes(&dir);
        assert_eq!(names[..PALETTES.len()], PALETTES.map(|(name, ..)| name));
        assert_eq!(names[PALETTES.len()..], ["mine", "plain"]);
        assert_eq!(palette_css(&dir, "mine", false), "light");
        assert_eq!(palette_css(&dir, "mine", true), "dark");
        assert_eq!(palette_css(&dir, "plain", true), "both"); // no dark variant: the same sheet
        assert_eq!(palette_css(&dir, "nord", true), "my nord"); // the user's file wins
        std::fs::remove_dir_all(dir).unwrap();
    }
}

//! `~/.config/spot/spot.conf`, read in one place: appearance, terminal and keys.
//!
//! The file is the source of truth. The palette writes to it (`set`) without touching the
//! user's other lines, and the directory watch in `style` re-reads it on every change.

use crate::style::{self, Appearance, Scheme, Style};
use crate::tr;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const CONFIG_FILE: &str = "spot.conf";

pub fn config_dir() -> PathBuf {
    glib::user_config_dir().join("spot")
}

pub fn config_path() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

// -- keys --------------------------------------------------------------------

/// What a key can do in the window. Enter is not here: it is the entry's own activation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Next,
    Previous,
    NextPage,
    PreviousPage,
    /// The second action of a row: the containing folder of a file…
    Alternate,
    /// Leave the palette, or close the window.
    Back,
    Settings,
    /// Launch the Nth row, from 1.
    Pick(u8),
}

pub type Accel = (gdk::Key, gdk::ModifierType);

/// The modifiers a binding can ask for, as written in the file and as shown in the window.
const MODIFIERS: [(&str, gdk::ModifierType, &str); 4] = [
    ("ctrl", gdk::ModifierType::CONTROL_MASK, "Ctrl+"),
    ("alt", gdk::ModifierType::ALT_MASK, "Alt+"),
    ("shift", gdk::ModifierType::SHIFT_MASK, "Shift+"),
    ("super", gdk::ModifierType::SUPER_MASK, "Super+"),
];

/// The modifiers of `state` that count: not Caps Lock, Num Lock or the pointer buttons.
pub fn modifiers(state: gdk::ModifierType) -> gdk::ModifierType {
    MODIFIERS
        .iter()
        .fold(gdk::ModifierType::empty(), |all, (_, mask, _)| all | *mask)
        & state
}

/// Every command: its name in `[Keys]` and its default accelerators.
pub fn default_keys() -> Vec<(String, Command, String)> {
    let fixed = [
        ("Next", Command::Next, "Down;<Ctrl>n;<Ctrl>j"),
        ("Previous", Command::Previous, "Up;<Ctrl>p;<Ctrl>k"),
        ("NextPage", Command::NextPage, "Page_Down"),
        ("PreviousPage", Command::PreviousPage, "Page_Up"),
        (
            "Alternate",
            Command::Alternate,
            "<Ctrl>Return;<Ctrl>KP_Enter",
        ),
        ("Back", Command::Back, "Escape"),
        ("Settings", Command::Settings, "<Ctrl>comma"),
    ];
    fixed
        .into_iter()
        .map(|(name, command, keys)| (name.to_owned(), command, keys.to_owned()))
        .chain((1..=9).map(|n| (format!("Pick{n}"), Command::Pick(n), format!("<Alt>{n}"))))
        .collect()
}

/// An accelerator the way GTK writes them: `<Ctrl>n`, `Page_Down`, `<Ctrl><Shift>comma`.
/// `gtk::accelerator_parse` does the same but wants a display, which the tests do not have.
fn parse_accel(text: &str) -> Option<Accel> {
    let mut rest = text.trim();
    let mut mods = gdk::ModifierType::empty();
    while let Some(tagged) = rest.strip_prefix('<') {
        let (name, after) = tagged.split_once('>')?;
        let name = name.to_ascii_lowercase();
        let name = match name.as_str() {
            "control" | "primary" => "ctrl",
            other => other,
        };
        mods |= MODIFIERS.iter().find(|(n, ..)| *n == name)?.1;
        rest = after;
    }
    Some((gdk::Key::from_name(rest)?.to_lower(), mods))
}

/// How an accelerator is shown in the footer and in the palette: `Ctrl+N`, `Ctrl+↵`, `↓`.
pub fn accel_label((key, mods): Accel) -> String {
    let mut label: String = MODIFIERS
        .iter()
        .filter(|(_, mask, _)| mods.contains(*mask))
        .map(|(.., shown)| *shown)
        .collect();
    match key {
        gdk::Key::Return | gdk::Key::KP_Enter => label.push('↵'),
        gdk::Key::Escape => label += "Esc",
        gdk::Key::Up => label.push('↑'),
        gdk::Key::Down => label.push('↓'),
        gdk::Key::Page_Up => label += "PgUp",
        gdk::Key::Page_Down => label += "PgDn",
        _ => match key.to_unicode().filter(|c| !c.is_control() && *c != ' ') {
            Some(c) => label.extend(c.to_uppercase()),
            None => label += &key.name().unwrap_or_default(),
        },
    }
    label
}

/// The bindings in effect, in the order of `default_keys`: the first match wins.
#[derive(Clone, Debug, PartialEq)]
pub struct Keymap(Vec<(Command, Accel)>);

impl Keymap {
    pub fn command(
        &self,
        key: gdk::Key,
        keycode: u32,
        state: gdk::ModifierType,
    ) -> Option<Command> {
        let (key, mods) = (key.to_lower(), modifiers(state));
        // The digit row also counts by position (keycodes 10 to 18 everywhere on Linux): on
        // AZERTY it types `&é"'` unshifted, and `<Alt>1` still has to be its first key.
        let digit = (10..=18)
            .contains(&keycode)
            .then(|| gdk::Key::from_name((keycode - 9).to_string().as_str()))
            .flatten();
        self.0
            .iter()
            .find(|(_, (k, m))| *m == mods && (*k == key || Some(*k) == digit))
            .map(|(command, _)| *command)
    }

    pub fn accels(&self, command: Command) -> impl Iterator<Item = Accel> + '_ {
        self.0
            .iter()
            .filter(move |(c, _)| *c == command)
            .map(|(_, accel)| *accel)
    }

    /// The first key of `command`, as shown to the user; `None` when it is unbound.
    pub fn label(&self, command: Command) -> Option<String> {
        self.accels(command).next().map(accel_label)
    }
}

// -- the file ----------------------------------------------------------------

/// What `spot.conf` asks for. A missing key keeps its default, and so does an invalid value,
/// so a typo never breaks the launcher: it lands in `errors` instead.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub appearance: Appearance,
    /// The terminal for `!`, with the argument that introduces the program.
    pub terminal: Option<String>,
    pub keys: Keymap,
    /// What is wrong in the file, each with a hint, in the user's language: shown in the window.
    pub errors: Vec<(String, String)>,
}

impl Config {
    /// `palettes` are the names `Palette` may take, see `style::palettes`.
    pub fn parse(data: &str, palettes: &[String]) -> Self {
        let keyfile = glib::KeyFile::new();
        let mut errors = vec![];
        if let Err(error) = keyfile.load_from_data(data, glib::KeyFileFlags::NONE) {
            errors.push((tr("spot.conf cannot be read"), error.message().to_owned()));
        }
        let get = |group: &str, key: &str| {
            let value = keyfile.string(group, key).ok()?;
            Some(value.trim().to_owned()).filter(|v| !v.is_empty())
        };
        let unknown = |what: &str, value: &str| {
            tr("spot.conf: unknown %s “%s”")
                .replacen("%s", what, 1)
                .replacen("%s", value, 1)
        };
        fn names<T>(all: &[(&str, T)]) -> String {
            let names: Vec<&str> = all.iter().map(|(name, _)| *name).collect();
            names.join(", ")
        }

        let mut appearance = Appearance::default();
        if let Some(value) = get("Appearance", "Style") {
            match Style::parse(&value) {
                Some(style) => appearance.style = style,
                None => errors.push((unknown("Style", &value), names(&Style::ALL))),
            }
        }
        if let Some(value) = get("Appearance", "ColorScheme") {
            match Scheme::parse(&value) {
                Some(scheme) => appearance.scheme = scheme,
                None => errors.push((unknown("ColorScheme", &value), names(&Scheme::ALL))),
            }
        }
        if let Some(value) = get("Appearance", "Palette").filter(|v| v != style::NO_PALETTE) {
            match palettes.iter().find(|p| p.eq_ignore_ascii_case(&value)) {
                Some(palette) => appearance.palette = Some(palette.clone()),
                None => errors.push((
                    unknown("Palette", &value),
                    format!("{}, {}", style::NO_PALETTE, palettes.join(", ")),
                )),
            }
        }

        let defaults = default_keys();
        for name in keyfile.keys("Keys").iter().flatten() {
            if !defaults.iter().any(|(known, ..)| known == name.as_str()) {
                let known: Vec<&str> = defaults.iter().map(|(n, ..)| n.as_str()).collect();
                errors.push((unknown("[Keys]", name.as_str()), known.join(", ")));
            }
        }
        let mut bindings = vec![];
        for (name, command, default) in &defaults {
            // a key that is there replaces the defaults, and an empty one unbinds the command
            let accels = keyfile
                .string("Keys", name)
                .map_or(default.clone(), |value| value.to_string());
            for accel in accels.split(';').map(str::trim).filter(|a| !a.is_empty()) {
                match parse_accel(accel) {
                    Some(accel) => bindings.push((*command, accel)),
                    None => errors.push((
                        tr("spot.conf: invalid shortcut “%s”").replacen("%s", accel, 1),
                        format!("[Keys] {name} · <Ctrl>n, <Alt>1, Page_Down, Escape…"),
                    )),
                }
            }
        }

        Config {
            appearance,
            terminal: get("Terminal", "Command"),
            keys: Keymap(bindings),
            errors,
        }
    }

    fn load() -> Self {
        // a missing file is the normal case: everything keeps its default
        let data = std::fs::read_to_string(config_path()).unwrap_or_default();
        Config::parse(&data, &style::palettes(&config_dir()))
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Rc<Config>>> = const { RefCell::new(None) };
}

/// The configuration in effect. Read from the file the first time, then kept in step with it
/// by `reload`, which the directory watch calls.
pub fn current() -> Rc<Config> {
    CURRENT.with_borrow_mut(|current| {
        current
            .get_or_insert_with(|| Rc::new(Config::load()))
            .clone()
    })
}

pub fn reload() -> Rc<Config> {
    let config = Rc::new(Config::load());
    for (error, hint) in &config.errors {
        eprintln!("spot: {error} ({hint})");
    }
    CURRENT.set(Some(config.clone()));
    config
}

/// Writes one setting to `spot.conf` and applies it.
pub fn set(group: &str, key: &str, value: &str) -> Result<(), glib::Error> {
    let path = config_path();
    ensure_file(&path)?;
    let data = std::fs::read_to_string(&path).map_err(io_error)?;
    // a file that does not parse is left alone: its owner has to see what is wrong with it first
    glib::KeyFile::new().load_from_data(&data, glib::KeyFileFlags::NONE)?;
    glib::file_set_contents(&path, with_setting(&data, group, key, value).as_bytes())?;
    style::refresh();
    Ok(())
}

/// Creates `spot.conf` from the commented template when there is none.
pub fn ensure_file(path: &Path) -> Result<(), glib::Error> {
    if path.exists() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io_error)?;
    }
    std::fs::write(path, template()).map_err(io_error)
}

fn io_error(error: std::io::Error) -> glib::Error {
    glib::Error::new(gio::IOErrorEnum::Failed, &error.to_string())
}

/// `data` with `key=value` in `[group]`: over the key's own line, else over the commented-out
/// one the template has, else at the end of the group. Every other line stays as it was, which
/// `glib::KeyFile` cannot promise: it would add the key after the group's trailing comments.
fn with_setting(data: &str, group: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = data.lines().map(String::from).collect();
    let header = format!("[{group}]");
    let start = match lines.iter().position(|line| line.trim() == header) {
        Some(index) => index + 1,
        None => {
            if lines.last().is_some_and(|line| !line.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.len()
        }
    };
    let end = lines[start..]
        .iter()
        .position(|line| line.starts_with('['))
        .map_or(lines.len(), |offset| start + offset);
    let is_key = |line: &str| {
        line.strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    };
    let line = format!("{key}={value}");
    let own = (start..end).find(|&i| is_key(lines[i].trim_start()));
    let commented = || {
        (start..end).find(|&i| {
            lines[i]
                .trim_start()
                .strip_prefix('#')
                .is_some_and(|rest| is_key(rest.trim_start()))
        })
    };
    match own.or_else(commented) {
        Some(index) => lines[index] = line,
        None => {
            // after the last line of the group that says something, before its closing blanks
            let last = (start..end).rfind(|&i| !lines[i].trim().is_empty());
            lines.insert(last.map_or(start, |i| i + 1), line);
        }
    }
    lines.join("\n") + "\n"
}

/// Every key with its default, commented out: what `spot.conf` starts as when the palette
/// creates it.
pub fn template() -> String {
    let palettes = style::palettes(Path::new("")).join(", ");
    let mut text = format!(
        "\
# spot settings. Save the file and the open window changes, nothing to restart.
# A line that starts with # is a comment; here they show the defaults.

[Appearance]
# {styles}
#Style=default
# system, light or dark
#ColorScheme=system
# {none}, {palettes}, or <name> for ~/.config/spot/themes/<name>.css
#Palette={none}

[Terminal]
# The terminal for `!`, then the argument that introduces the program
#Command=alacritty -e

[Keys]
# Written as GTK does: <Ctrl>n, <Alt>1, Page_Down. Several keys are separated by ;
",
        none = style::NO_PALETTE,
        styles = Style::ALL.map(|(name, _)| name).join(", ")
    );
    for (name, _, keys) in default_keys() {
        text += &format!("#{name}={keys}\n");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdk::ModifierType as M;

    fn parse(data: &str) -> Config {
        Config::parse(data, &style::palettes(Path::new("")))
    }

    #[test]
    fn empty_file_keeps_the_defaults() {
        let config = parse("");
        assert_eq!(config.appearance, Appearance::default());
        assert_eq!(config.terminal, None);
        assert!(config.errors.is_empty());
        assert_eq!(config.keys.label(Command::Settings).unwrap(), "Ctrl+,");
    }

    #[test]
    fn reads_every_group() {
        let config = parse(
            "[Appearance]\nStyle = Compact \nColorScheme=LIGHT\nPalette=Nord\n\
             [Terminal]\nCommand= foot \n[Keys]\nNext=Tab\n",
        );
        let a = &config.appearance;
        assert_eq!((a.style, a.scheme), (Style::Compact, Scheme::Light));
        assert_eq!(a.palette.as_deref(), Some("nord"));
        assert_eq!(config.terminal.as_deref(), Some("foot"));
        assert!(config.errors.is_empty(), "{:?}", config.errors);
        assert_eq!(
            parse("[Appearance]\nPalette=none\n").appearance.palette,
            None
        );
    }

    #[test]
    fn a_typo_keeps_the_default_for_that_key_only_and_is_reported() {
        let config = parse("[Appearance]\nStyle=glas\nColorScheme=dark\nPalette=nope\n");
        let a = &config.appearance;
        assert_eq!((a.style, a.scheme), (Style::Default, Scheme::Dark));
        assert_eq!(a.palette, None);
        assert_eq!(config.errors.len(), 2);
        assert!(config.errors[0].0.contains("glas"));
        assert!(config.errors[0].1.starts_with("default, glass, compact"));
    }

    #[test]
    fn broken_file_is_reported_and_keeps_the_defaults() {
        let config = parse("not a keyfile");
        assert_eq!(config.appearance, Appearance::default());
        assert_eq!(config.keys, parse("").keys);
        assert_eq!(config.errors.len(), 1);
    }

    #[test]
    fn accelerators() {
        assert_eq!(parse_accel("<Ctrl>n"), Some((gdk::Key::n, M::CONTROL_MASK)));
        assert_eq!(parse_accel("<Control>N"), parse_accel("<ctrl>n"));
        assert_eq!(
            parse_accel(" <Ctrl><Shift>comma "),
            Some((gdk::Key::comma, M::CONTROL_MASK | M::SHIFT_MASK))
        );
        assert_eq!(
            parse_accel("Page_Down"),
            Some((gdk::Key::Page_Down, M::empty()))
        );
        for wrong in ["", "<Ctrl>", "<Hyper>n", "<Ctrl", "nosuchkey", ","] {
            assert_eq!(parse_accel(wrong), None, "{wrong}");
        }
        assert_eq!(accel_label(parse_accel("<Ctrl>Return").unwrap()), "Ctrl+↵");
        assert_eq!(
            accel_label(parse_accel("<Alt><Shift>j").unwrap()),
            "Alt+Shift+J"
        );
        assert_eq!(accel_label(parse_accel("Down").unwrap()), "↓");
        assert_eq!(accel_label(parse_accel("F2").unwrap()), "F2");
    }

    #[test]
    fn default_keys_find_their_command() {
        let keys = parse("").keys;
        let press = |key, keycode, state| keys.command(key, keycode, state);
        assert_eq!(press(gdk::Key::Down, 116, M::empty()), Some(Command::Next));
        assert_eq!(press(gdk::Key::n, 57, M::CONTROL_MASK), Some(Command::Next));
        // Caps Lock gives the upper-case key and its own modifier: still Ctrl+P
        assert_eq!(
            press(gdk::Key::P, 33, M::CONTROL_MASK | M::LOCK_MASK),
            Some(Command::Previous)
        );
        assert_eq!(press(gdk::Key::Escape, 9, M::empty()), Some(Command::Back));
        assert_eq!(
            press(gdk::Key::Return, 36, M::CONTROL_MASK),
            Some(Command::Alternate)
        );
        assert_eq!(press(gdk::Key::_3, 12, M::ALT_MASK), Some(Command::Pick(3)));
        // AZERTY: the third key of the digit row types a quote
        assert_eq!(
            press(gdk::Key::quotedbl, 12, M::ALT_MASK),
            Some(Command::Pick(3))
        );
        // typing is nobody's command
        for (key, state) in [
            (gdk::Key::n, M::empty()),
            (gdk::Key::Return, M::empty()),
            (gdk::Key::_3, M::empty()),
            (gdk::Key::n, M::CONTROL_MASK | M::SHIFT_MASK),
        ] {
            assert_eq!(press(key, 0, state), None, "{key:?}");
        }
    }

    #[test]
    fn keys_group_replaces_the_defaults_of_the_commands_it_names() {
        let config = parse("[Keys]\nNext=Tab;<Ctrl>d\nSettings=\nPick1=<Super>a\n");
        assert!(config.errors.is_empty(), "{:?}", config.errors);
        let keys = &config.keys;
        assert_eq!(
            keys.command(gdk::Key::Tab, 23, M::empty()),
            Some(Command::Next)
        );
        assert_eq!(keys.command(gdk::Key::Down, 116, M::empty()), None);
        assert_eq!(keys.label(Command::Settings), None); // unbound
        assert_eq!(keys.label(Command::Pick(1)).unwrap(), "Super+A");
        assert_eq!(keys.label(Command::Previous).unwrap(), "↑"); // untouched
    }

    #[test]
    fn wrong_keys_are_reported() {
        let config = parse("[Keys]\nNxt=Down\nNext=<Ctrl>n;<Hyper>x\n");
        assert_eq!(config.errors.len(), 2, "{:?}", config.errors);
        assert!(config.errors[0].0.contains("Nxt"));
        assert!(config.errors[1].0.contains("<Hyper>x"));
        // the valid half of the line still counts
        assert_eq!(
            config.keys.command(gdk::Key::n, 57, M::CONTROL_MASK),
            Some(Command::Next)
        );
    }

    /// The template is the documentation of the file: uncommented, it has to be the defaults.
    #[test]
    fn template_shows_the_defaults() {
        let template = template();
        assert_eq!(parse(&template), parse("")); // all comments
        let uncommented: String = template
            .lines()
            .map(|line| match line.strip_prefix('#') {
                Some(setting) if setting.contains('=') && !setting.starts_with(' ') => setting,
                _ => line,
            })
            .map(|line| format!("{line}\n"))
            .collect();
        let config = parse(&uncommented);
        assert!(config.errors.is_empty(), "{:?}", config.errors);
        assert_eq!(config.appearance, Appearance::default());
        assert_eq!(config.keys, parse("").keys);
        assert_eq!(config.terminal.as_deref(), Some("alacritty -e"));
    }

    #[test]
    fn a_setting_is_written_without_disturbing_the_file() {
        // over the commented-out default of the template
        let text = with_setting(&template(), "Appearance", "Style", "glass");
        assert!(text.contains("mono\nStyle=glass\n# system"));
        assert_eq!(text.lines().count(), template().lines().count());
        assert_eq!(parse(&text).appearance.style, Style::Glass);
        // over its own line, comments and other keys kept
        let mine = "# mine\n[Appearance]\nStyle = glass\n# keep\nColorScheme=dark\n\n[Terminal]\nCommand=foot\n";
        assert_eq!(
            with_setting(mine, "Appearance", "Style", "compact"),
            mine.replace("Style = glass", "Style=compact")
        );
        // a new key goes at the end of its group, a new group at the end of the file
        assert_eq!(
            with_setting(mine, "Appearance", "Palette", "nord"),
            mine.replace("dark\n", "dark\nPalette=nord\n")
        );
        assert_eq!(
            with_setting("[Terminal]\nCommand=foot\n", "Appearance", "Style", "glass"),
            "[Terminal]\nCommand=foot\n\n[Appearance]\nStyle=glass\n"
        );
        assert_eq!(
            with_setting("", "Appearance", "Style", "glass"),
            "[Appearance]\nStyle=glass\n"
        );
        // `StyleSheet=` is another key
        assert_eq!(
            with_setting(
                "[Appearance]\nStyleSheet=x\n",
                "Appearance",
                "Style",
                "glass"
            ),
            "[Appearance]\nStyleSheet=x\nStyle=glass\n"
        );
    }

    /// The examples in the README are what users copy: they must parse without a complaint.
    #[test]
    fn readme_examples_parse() {
        let readme = include_str!("../README.md");
        let example = |group: &str| {
            let block = readme
                .split("```ini\n")
                .skip(1) // what comes before the first example is prose
                .filter_map(|rest| rest.split("```").next())
                .find(|block| block.contains(group))
                .unwrap_or_else(|| panic!("a {group} example in the README"));
            let config = parse(block);
            assert!(config.errors.is_empty(), "{group}: {:?}", config.errors);
            config
        };
        let a = example("[Appearance]").appearance;
        assert_eq!((a.style, a.scheme), (Style::Glass, Scheme::System));
        assert_eq!(a.palette.as_deref(), Some("catppuccin"));
        assert!(example("[Terminal]").terminal.is_some());
        let keys = example("[Keys]").keys;
        assert_eq!(
            keys.command(gdk::Key::F2, 68, M::empty()),
            Some(Command::Settings)
        );
        assert_eq!(keys.command(gdk::Key::j, 44, M::CONTROL_MASK), None); // replaced
    }
}

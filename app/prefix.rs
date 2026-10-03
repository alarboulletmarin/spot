//! Prefixes that turn the query into an action instead of a search:
//!
//! - `!ls -la` runs the command in a terminal;
//! - `search: rust gtk` (or `g:`, `yt:`, `gh:`…) opens a web search in the default browser;
//! - an address (`https://…`, `www.…`) opens in the default browser.
//!
//! A prefix claims the whole query: while it is there, nothing else is searched.

use crate::results::{Hit, themed};
use crate::search::home;
use crate::style::{CONFIG_FILE, config_dir};
use crate::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

/// A web search: what follows the keyword replaces `%s` in the address.
#[derive(Clone, Debug, PartialEq)]
struct Engine {
    keyword: String,
    address: String,
}

/// `search:` is the one `spot.conf` is most likely to change; the others are shortcuts.
const BUILT_IN_ENGINES: [(&str, &str); 5] = [
    ("search", "https://duckduckgo.com/?q=%s"),
    ("ddg", "https://duckduckgo.com/?q=%s"),
    ("g", "https://www.google.com/search?q=%s"),
    ("yt", "https://www.youtube.com/results?search_query=%s"),
    ("gh", "https://github.com/search?q=%s"),
];

/// What the query asks for.
#[derive(Debug, PartialEq)]
enum Intent {
    Terminal(String),
    Web { address: String, text: String },
    Url(String),
}

/// The rows for a query that starts with a prefix, `None` for an ordinary query.
///
/// `Some(vec![])` means the prefix is there but nothing follows it yet: the query is still
/// claimed, so that `!` alone does not go looking for files named `!`.
pub fn hits(query: &str) -> Option<Vec<Hit>> {
    Some(match parse(query, engines)? {
        Intent::Terminal(command) if !command.is_empty() => vec![terminal_hit(command)],
        Intent::Web { address, text } if !text.is_empty() => vec![web_hit(&address, &text)],
        Intent::Url(url) => vec![url_hit(url)],
        _ => vec![],
    })
}

/// The last resort when nothing matched: search the web for the whole query, with the engine
/// of `search:`.
pub fn web_fallback(query: &str) -> Option<Hit> {
    fallback_hit(query, &engines())
}

fn fallback_hit(query: &str, engines: &[Engine]) -> Option<Hit> {
    let query = query.trim();
    let engine = engines.iter().find(|e| e.keyword == "search")?;
    (!query.is_empty()).then(|| web_hit(&engine.address, query))
}

/// `engines` is only called for a `keyword:` query, so that a plain search never reads the file.
fn parse(query: &str, engines: impl FnOnce() -> Vec<Engine>) -> Option<Intent> {
    let query = query.trim();
    if let Some(command) = query.strip_prefix('!') {
        return Some(Intent::Terminal(command.trim().to_owned()));
    }
    if let Some(url) = web_address(query) {
        return Some(Intent::Url(url));
    }
    let (keyword, text) = query.split_once(':')?;
    // starts with a letter, so that `2:30` and `10:1` are left to the calculator
    let mut chars = keyword.chars();
    let is_keyword = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !is_keyword {
        return None;
    }
    let engine = engines()
        .into_iter()
        .find(|e| e.keyword.eq_ignore_ascii_case(keyword))?;
    Some(Intent::Web {
        address: engine.address,
        text: text.trim().to_owned(),
    })
}

/// An address typed in full: a scheme, or `www.` (opened as https). Not a bare `name.tld`:
/// `notes.md` and `main.rs` are file names, and those suffixes are also domains.
fn web_address(query: &str) -> Option<String> {
    if query.contains(char::is_whitespace) {
        return None;
    }
    let lower = query.to_ascii_lowercase();
    for scheme in ["http://", "https://"] {
        if lower.starts_with(scheme) {
            return (query.len() > scheme.len()).then(|| query.to_owned());
        }
    }
    let host = lower.strip_prefix("www.")?;
    (host.len() > 2 && host.contains('.') && !host.starts_with('.'))
        .then(|| format!("https://{query}"))
}

// -- web searches ----------------------------------------------------------

/// The engines in `spot.conf`, then the built-in ones: the first match wins, so the file can
/// redefine a keyword.
fn engines() -> Vec<Engine> {
    let mut engines = config_engines(&read_config());
    engines.extend(BUILT_IN_ENGINES.iter().map(|&(keyword, address)| Engine {
        keyword: keyword.into(),
        address: address.into(),
    }));
    engines
}

/// Reads the `[Search]` group. A broken entry is reported and skipped, never fatal.
fn config_engines(keyfile: &glib::KeyFile) -> Vec<Engine> {
    let Ok(keys) = keyfile.keys("Search") else {
        return vec![];
    };
    let mut engines = vec![];
    for keyword in keys {
        let Ok(address) = keyfile.string("Search", &keyword) else {
            continue;
        };
        let address = address.trim();
        let is_web = address.starts_with("http://") || address.starts_with("https://");
        if is_web && address.contains("%s") {
            engines.push(Engine {
                keyword: keyword.to_string(),
                address: address.to_owned(),
            });
        } else {
            eprintln!(
                "spot: ignoring “{keyword}” in {CONFIG_FILE}: the address must start with http:// or https:// and contain %s"
            );
        }
    }
    engines
}

/// The address to open: `text` goes where the template has `%s`, escaped so that `&`, `#` and
/// `+` stay part of the search.
fn search_url(address: &str, text: &str) -> String {
    address.replace("%s", &glib::Uri::escape_string(text, None, false))
}

/// The default browser's own icon, or a generic one.
fn browser_icon() -> gio::Icon {
    gio::AppInfo::default_for_uri_scheme("https")
        .and_then(|browser| browser.icon())
        .unwrap_or_else(|| themed("web-browser-symbolic"))
}

fn open_url(url: String) -> crate::results::Action {
    Box::new(move |ctx| gio::AppInfo::launch_default_for_uri(&url, Some(ctx)))
}

fn web_hit(address: &str, text: &str) -> Hit {
    let url = search_url(address, text);
    let host = glib::Uri::parse(&url, glib::UriFlags::NONE)
        .ok()
        .and_then(|uri| uri.host())
        .map_or(String::new(), |host| {
            host.trim_start_matches("www.").to_owned()
        });
    Hit {
        score: 0,
        title: tr("Search for “%s”").replacen("%s", text, 1),
        subtitle: host,
        kind: tr("Web"),
        icon: browser_icon(),
        activate: open_url(url),
        alt: None,
        path: None,
    }
}

fn url_hit(url: String) -> Hit {
    Hit {
        score: 0,
        title: url.clone(),
        subtitle: tr("Open in the default browser"),
        kind: tr("Web"),
        icon: browser_icon(),
        activate: open_url(url),
        alt: None,
        path: None,
    }
}

// -- terminal --------------------------------------------------------------

/// How each known terminal is told to run a program, as the arguments that go between its name
/// and the program. Anything not listed gets `-e`, which most terminals follow after xterm.
const TERMINALS: [(&str, &[&str]); 16] = [
    ("xdg-terminal-exec", &[]),
    ("x-terminal-emulator", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("ptyxis", &["--"]),
    ("kgx", &["-e"]),
    ("konsole", &["-e"]),
    ("xfce4-terminal", &["-x"]),
    ("mate-terminal", &["-x"]),
    ("tilix", &["-e"]),
    ("terminator", &["-x"]),
    ("alacritty", &["-e"]),
    ("kitty", &[]),
    ("foot", &[]),
    ("wezterm", &["start", "--"]),
    ("lxterminal", &["-e"]),
    ("xterm", &["-e"]),
];

fn terminal_hit(command: String) -> Hit {
    let (keep_open, close) = (command.clone(), command.clone());
    Hit {
        score: 0,
        title: tr("Run “%s” in a terminal").replacen("%s", &command, 1),
        subtitle: tr("The terminal stays open · Ctrl+Enter closes it when the command ends"),
        kind: tr("Terminal"),
        icon: themed("utilities-terminal-symbolic"),
        activate: Box::new(move |ctx| run_in_terminal(&keep_open, true, Some(ctx))),
        alt: Some(Box::new(move |ctx| {
            run_in_terminal(&close, false, Some(ctx))
        })),
        path: None,
    }
}

/// Opens the terminal and runs `command` in it; see `shell_argv` for `keep_open`.
fn run_in_terminal(
    command: &str,
    keep_open: bool,
    ctx: Option<&gdk::AppLaunchContext>,
) -> Result<(), glib::Error> {
    let mut argv = find_terminal().ok_or_else(|| {
        glib::Error::new(
            gio::IOErrorEnum::NotFound,
            &tr("no terminal found, set one in spot.conf"),
        )
    })?;
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    argv.extend(shell_argv(&shell, command, keep_open));

    // An application entry made on the fly: it gives the launch a start-up notification, which
    // is what lets the new window take focus under Wayland, and a working directory.
    let entry = glib::KeyFile::new();
    let group = "Desktop Entry";
    entry.set_string(group, "Type", "Application");
    entry.set_string(group, "Name", command);
    entry.set_string(group, "Exec", &exec_line(&argv));
    entry.set_boolean(group, "StartupNotify", true);
    if !home().is_empty() {
        entry.set_string(group, "Path", home());
    }
    let info = gio_unix::DesktopAppInfo::from_keyfile(&entry).ok_or_else(|| {
        glib::Error::new(gio::IOErrorEnum::InvalidData, "invalid terminal command")
    })?;
    info.launch(&[], ctx)
}

/// The user's shell, interactive so that aliases and the PATH of a terminal session apply,
/// running `command`; `keep_open` ends with a new shell so the output is still there.
fn shell_argv(shell: &str, command: &str, keep_open: bool) -> Vec<String> {
    let script = if keep_open {
        // a newline, not `;`: a trailing `# comment` would swallow the `exec`
        format!(
            "{command}\nexec {}",
            glib::shell_quote(shell).to_string_lossy()
        )
    } else {
        command.to_owned()
    };
    vec![shell.to_owned(), "-ic".to_owned(), script]
}

/// An `Exec=` line that runs `argv` as is. GLib expands `%` codes in it, so a literal `%`
/// (`date +%F`) has to be doubled or it is silently dropped.
fn exec_line(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| glib::shell_quote(arg).to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
        .replace('%', "%%")
}

/// The terminal to use, with the arguments that precede the program: the one named in
/// `spot.conf`, else the system's default.
fn find_terminal() -> Option<Vec<String>> {
    let configured = read_config().string("Terminal", "Command").ok();
    if let Some(command) = configured
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        match resolve_terminal(command) {
            Some(argv) => return Some(argv),
            None => eprintln!("spot: terminal “{command}” from {CONFIG_FILE} not found"),
        }
    }
    default_terminals()
        .into_iter()
        .find_map(|c| resolve_terminal(&c))
}

/// In order of preference: the XDG default-terminal tool, `$TERMINAL` (set on purpose, on Sway,
/// Hyprland, i3…), GNOME's setting, Debian's alternative, then whatever is installed.
///
/// `$TERMINAL` comes before GNOME's setting: that schema is installed almost everywhere and
/// says `gnome-terminal` until someone changes it, which would hide a deliberate choice.
fn default_terminals() -> Vec<String> {
    const SCHEMA: &str = "org.gnome.desktop.default-applications.terminal";
    let gnome = gio::SettingsSchemaSource::default()
        .is_some_and(|source| source.lookup(SCHEMA, true).is_some())
        .then(|| gio::Settings::new(SCHEMA).string("exec").to_string());
    terminal_candidates(std::env::var("TERMINAL").ok(), gnome)
}

fn terminal_candidates(env: Option<String>, gnome: Option<String>) -> Vec<String> {
    let mut candidates = vec!["xdg-terminal-exec".to_owned()];
    candidates.extend(env.filter(|terminal| !terminal.trim().is_empty()));
    candidates.extend(gnome.filter(|terminal| !terminal.trim().is_empty()));
    candidates.extend(TERMINALS.iter().map(|(name, _)| name.to_string()));
    candidates
}

/// `command` is a terminal's name, or its name followed by the arguments to run a program
/// (`alacritty -e`). `None` when the program is not installed.
fn resolve_terminal(command: &str) -> Option<Vec<String>> {
    let mut argv: Vec<String> = glib::shell_parse_argv(command)
        .ok()?
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let program = argv.first()?.clone();
    glib::find_program_in_path(&program)?;
    if argv.len() == 1 {
        let name = std::path::Path::new(&program).file_name()?.to_str()?;
        let flags = TERMINALS.iter().find(|(known, _)| *known == name);
        argv.extend(flags.map_or(vec!["-e".to_owned()], |(_, flags)| {
            flags.iter().map(|f| f.to_string()).collect()
        }));
    }
    Some(argv)
}

/// `spot.conf`, or an empty key file: a broken one is already reported by the style code.
fn read_config() -> glib::KeyFile {
    let keyfile = glib::KeyFile::new();
    if keyfile
        .load_from_file(config_dir().join(CONFIG_FILE), glib::KeyFileFlags::NONE)
        .is_err()
    {
        return glib::KeyFile::new();
    }
    keyfile
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn built_in() -> Vec<Engine> {
        BUILT_IN_ENGINES
            .iter()
            .map(|&(keyword, address)| Engine {
                keyword: keyword.into(),
                address: address.into(),
            })
            .collect()
    }

    fn parse_with_built_ins(query: &str) -> Option<Intent> {
        parse(query, built_in)
    }

    fn keyfile(data: &str) -> glib::KeyFile {
        let keyfile = glib::KeyFile::new();
        keyfile
            .load_from_data(data, glib::KeyFileFlags::NONE)
            .unwrap();
        keyfile
    }

    #[test]
    fn bang_runs_in_a_terminal() {
        assert_eq!(
            parse_with_built_ins("!ls -la"),
            Some(Intent::Terminal("ls -la".into()))
        );
        assert_eq!(
            parse_with_built_ins("!  git status "),
            Some(Intent::Terminal("git status".into()))
        );
        // claimed, with nothing to run yet
        assert_eq!(
            parse_with_built_ins("!"),
            Some(Intent::Terminal(String::new()))
        );
        assert!(hits("!").unwrap().is_empty());
    }

    #[test]
    fn keyword_opens_a_web_search() {
        let web = |address: &str, text: &str| {
            Some(Intent::Web {
                address: address.into(),
                text: text.into(),
            })
        };
        let ddg = "https://duckduckgo.com/?q=%s";
        assert_eq!(
            parse_with_built_ins("search: rust gtk"),
            web(ddg, "rust gtk")
        );
        assert_eq!(parse_with_built_ins("search:rust"), web(ddg, "rust"));
        assert_eq!(parse_with_built_ins("SEARCH:  rust "), web(ddg, "rust"));
        assert_eq!(
            parse_with_built_ins("gh: alarboulletmarin/spot"),
            web("https://github.com/search?q=%s", "alarboulletmarin/spot")
        );
        // the keyword alone is claimed too, with nothing to search for yet
        assert_eq!(
            parse_with_built_ins("g:"),
            web("https://www.google.com/search?q=%s", "")
        );
    }

    #[test]
    fn ordinary_queries_are_left_alone() {
        let never = || -> Vec<Engine> { panic!("the config was read for a plain query") };
        for query in [
            "firefox",
            "report 2024",
            "notes.md",
            "a b: c",
            "",
            "5!",
            "2:30",
        ] {
            assert_eq!(parse(query, never), None, "{query}");
        }
        // looks like a keyword, is not one
        for query in ["note: buy milk", "http:foo", ":x", "a/b: c"] {
            assert_eq!(parse_with_built_ins(query), None, "{query}");
        }
    }

    #[test]
    fn addresses_open_in_the_browser() {
        let url = |u: &str| Some(Intent::Url(u.into()));
        assert_eq!(
            parse_with_built_ins("https://example.com/a?b=1#c"),
            url("https://example.com/a?b=1#c")
        );
        assert_eq!(
            parse_with_built_ins("HTTP://example.com"),
            url("HTTP://example.com")
        );
        assert_eq!(
            parse_with_built_ins("www.example.com"),
            url("https://www.example.com")
        );
        // not addresses
        for query in [
            "https://",
            "https://a b",
            "www.",
            "www.localhost",
            "example.com",
            "main.rs",
        ] {
            assert_eq!(parse_with_built_ins(query), None, "{query}");
        }
    }

    #[test]
    fn config_engines_come_first() {
        let file = keyfile(
            "[Search]\nsearch=https://www.startpage.com/do/search?q=%s\nnix=https://search.nixos.org/packages?query=%s\n",
        );
        let mut engines = config_engines(&file);
        engines.extend(built_in());
        let found = |kw: &str| parse(&format!("{kw}: x"), || engines.clone());
        let address = |intent| match intent {
            Some(Intent::Web { address, .. }) => address,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            address(found("search")),
            "https://www.startpage.com/do/search?q=%s"
        );
        assert_eq!(
            address(found("nix")),
            "https://search.nixos.org/packages?query=%s"
        );
        assert_eq!(address(found("g")), "https://www.google.com/search?q=%s"); // untouched
    }

    #[test]
    fn broken_engines_are_skipped() {
        let file = keyfile(
            "[Search]\nno-placeholder=https://example.com/\nnot-web=file:///etc/passwd?%s\nempty=\nok=https://example.com/?q=%s\n",
        );
        let keywords: Vec<_> = config_engines(&file)
            .into_iter()
            .map(|e| e.keyword)
            .collect();
        assert_eq!(keywords, ["ok"]);
        assert!(config_engines(&keyfile("[Appearance]\nStyle=glass\n")).is_empty());
    }

    #[test]
    fn the_fallback_uses_the_engine_of_search() {
        let host =
            |engines: &[Engine]| fallback_hit(" rust gtk ", engines).map(|h| (h.title, h.subtitle));
        assert_eq!(
            host(&built_in()),
            Some((
                tr("Search for “%s”").replacen("%s", "rust gtk", 1),
                "duckduckgo.com".into()
            ))
        );
        let mut engines = config_engines(&keyfile(
            "[Search]\nsearch=https://www.startpage.com/do/search?q=%s\n",
        ));
        engines.extend(built_in());
        assert_eq!(host(&engines).unwrap().1, "startpage.com");
        assert!(fallback_hit("  ", &built_in()).is_none());
        assert!(fallback_hit("x", &[]).is_none());
    }

    #[test]
    fn the_search_text_is_escaped() {
        let address = "https://example.com/?q=%s";
        assert_eq!(search_url(address, "a b"), "https://example.com/?q=a%20b");
        assert_eq!(
            search_url(address, "c++ & x=1 #2 100%"),
            "https://example.com/?q=c%2B%2B%20%26%20x%3D1%20%232%20100%25"
        );
        assert_eq!(
            search_url(address, "café"),
            "https://example.com/?q=caf%C3%A9"
        );
        // a literal %s typed by the user is not replaced a second time
        assert_eq!(search_url(address, "%s"), "https://example.com/?q=%25s");
    }

    #[test]
    fn rows_say_what_they_do() {
        let terminal = hits("!ls").unwrap().remove(0);
        assert_eq!(terminal.kind, tr("Terminal"));
        assert!(terminal.alt.is_some()); // Ctrl+Enter: close when done
        // built directly: `hits("g: rust")` would read the real spot.conf of whoever runs the tests
        let web = web_hit("https://www.google.com/search?q=%s", "rust");
        assert_eq!((web.kind, web.subtitle.as_str()), (tr("Web"), "google.com"));
        let url = hits("https://example.com").unwrap().remove(0);
        assert_eq!(url.title, "https://example.com");
        assert!(hits("report").is_none());
    }

    #[test]
    fn terminal_arguments() {
        let sh = |s: &str| s.to_owned();
        assert_eq!(
            shell_argv("/bin/bash", "echo hi", true),
            [sh("/bin/bash"), sh("-ic"), sh("echo hi\nexec '/bin/bash'")]
        );
        assert_eq!(
            shell_argv("/bin/bash", "echo hi", false),
            [sh("/bin/bash"), sh("-ic"), sh("echo hi")]
        );
        // a comment must not take the shell that follows with it
        assert!(shell_argv("sh", "ls # all", true)[2].ends_with("\nexec 'sh'"));
    }

    #[test]
    fn terminal_preference_order() {
        let names = |env: Option<&str>, gnome: Option<&str>| {
            terminal_candidates(env.map(String::from), gnome.map(String::from))
        };
        let candidates = names(Some("foot"), Some("gnome-terminal"));
        assert_eq!(
            candidates[..3],
            ["xdg-terminal-exec", "foot", "gnome-terminal"]
        );
        assert_eq!(names(None, None)[0], "xdg-terminal-exec");
        assert_eq!(names(Some(" "), Some("")).len(), 1 + TERMINALS.len()); // empty values are ignored
        // every known terminal is a candidate, once the specific ones are out of the way
        assert!(names(None, None).contains(&"kitty".to_owned()));
    }

    #[test]
    fn known_terminals_get_their_flags() {
        // `sh` stands in for a terminal that is certainly installed
        assert_eq!(resolve_terminal("sh"), Some(vec!["sh".into(), "-e".into()]));
        assert_eq!(
            resolve_terminal("sh -x"),
            Some(vec!["sh".into(), "-x".into()])
        ); // explicit wins
        assert_eq!(resolve_terminal("no-such-terminal-xyz"), None);
        assert_eq!(resolve_terminal(""), None);
        for (name, _) in TERMINALS {
            assert!(!name.contains(char::is_whitespace));
        }
    }

    /// The whole chain with a stand-in terminal: a script that records the arguments it is given.
    /// A `%`, quotes and spaces must reach the shell untouched.
    #[test]
    fn the_command_reaches_the_shell_intact() {
        let dir = std::env::temp_dir().join(format!("spot-prefix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let terminal = dir.join("fake-terminal");
        let record = dir.join("args");
        std::fs::write(
            &terminal,
            format!(
                "#!/bin/sh\nshift # the flag\nprintf '%s\\n' \"$@\" > '{0}.tmp' && mv '{0}.tmp' '{0}'\n",
                record.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(
            &terminal,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();

        let mut argv = resolve_terminal(&format!("{} -e", terminal.display())).unwrap();
        let command = r#"date +%F; echo "it's 100%" 'a b'"#;
        argv.extend(shell_argv("/bin/bash", command, true));
        let entry = glib::KeyFile::new();
        entry.set_string("Desktop Entry", "Type", "Application");
        entry.set_string("Desktop Entry", "Name", "test");
        entry.set_string("Desktop Entry", "Exec", &exec_line(&argv));
        gio_unix::DesktopAppInfo::from_keyfile(&entry)
            .unwrap()
            .launch(&[], gio::AppLaunchContext::NONE)
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while !record.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let received = std::fs::read_to_string(&record).expect("the terminal was not run");
        assert_eq!(
            received,
            format!("/bin/bash\n-ic\n{command}\nexec '/bin/bash'\n")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The README's example is what users copy: it has to be valid.
    #[test]
    fn readme_example_parses() {
        let readme = include_str!("../README.md");
        let block = readme
            .split("```ini\n")
            .skip(1) // what comes before the first example is prose, which may mention the group
            .filter_map(|rest| rest.split("```").next())
            .find(|block| block.contains("[Search]"))
            .expect("a [Search] example in the README");
        let file = keyfile(block);
        assert!(!config_engines(&file).is_empty());
        assert!(file.string("Terminal", "Command").is_ok());
    }
}

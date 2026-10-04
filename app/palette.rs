//! The command palette: spot's own settings and shortcuts, as result rows.
//!
//! `>` lists them all and what follows narrows the list down (`>dark`, `>keys`). A whole word
//! does it too in an ordinary search: `theme`, `settings`, `shortcuts`, or a value (`glass`).
//!
//! A setting is previewed while its row is selected, and Enter writes it to `spot.conf`: the
//! file stays the only place where settings live.

use crate::config::{self, Command, Config};
use crate::results::{Hit, Then, themed};
use crate::search::home;
use crate::style::{self, Appearance, Scheme, Style};
use crate::tr;
use gtk::gio;
use gtk::prelude::*;

pub const PREFIX: char = '>';

/// The rows for `>filter`: every row whose title or keywords have a word starting with each
/// word of the filter.
pub fn rows(filter: &str) -> Vec<Hit> {
    let palettes = style::palettes(&config::config_dir());
    select(all(&config::current(), &palettes, false), filter, false)
}

/// The rows an ordinary query brings up: it has to be one of their keywords, in full, so that
/// typing towards an application's name does not fill the list with settings.
pub fn word_rows(query: &str) -> Vec<Hit> {
    let palettes = style::palettes(&config::config_dir());
    select(all(&config::current(), &palettes, true), query, true)
}

/// What is wrong in `spot.conf`, said in the window: nobody reads a resident process's stderr.
pub fn error_rows() -> Vec<Hit> {
    let errors = &config::current().errors;
    let row = |(error, hint): &(String, String)| Hit {
        title: error.clone(),
        subtitle: hint.clone(),
        kind: tr("Settings"),
        icon: themed("dialog-warning-symbolic"),
        ..open_config()
    };
    errors.iter().map(row).collect()
}

/// A row and the keywords that find it, besides its title.
type Row = (String, Hit);

fn select(rows: Vec<Row>, filter: &str, whole_words: bool) -> Vec<Hit> {
    let filter = filter.to_lowercase();
    let wanted: Vec<&str> = filter.split_whitespace().collect();
    if whole_words && wanted.is_empty() {
        return vec![];
    }
    rows.into_iter()
        .filter(|(keywords, hit)| {
            let text = if whole_words {
                keywords.to_lowercase()
            } else {
                format!("{} {keywords}", hit.title).to_lowercase()
            };
            let words: Vec<&str> = text
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .collect();
            wanted.iter().all(|want| {
                words.iter().any(|word| {
                    if whole_words {
                        word == want
                    } else {
                        word.starts_with(want)
                    }
                })
            })
        })
        .map(|(_, hit)| hit)
        .collect()
}

/// Every row. `in_search` adds the one that leads to the palette, for an ordinary search.
fn all(config: &Config, palettes: &[String], in_search: bool) -> Vec<Row> {
    let now = &config.appearance;
    let theme = tr("theme style appearance");
    let mut rows = vec![];

    for (name, style) in Style::ALL {
        let shown = Appearance {
            style,
            ..now.clone()
        };
        let hit = setting(tr("Style"), "Style", name, now.style == style, shown);
        rows.push((format!("{name} {theme}"), hit));
    }
    for (name, scheme) in Scheme::ALL {
        let shown = Appearance {
            scheme,
            ..now.clone()
        };
        let hit = setting(
            tr("Colour scheme"),
            "ColorScheme",
            name,
            now.scheme == scheme,
            shown,
        );
        rows.push((format!("{name} {theme}"), hit));
    }
    let none = [style::NO_PALETTE.to_owned()];
    for name in none.iter().chain(palettes) {
        let palette = (name != style::NO_PALETTE).then(|| name.clone());
        let current = now.palette == palette;
        let shown = Appearance {
            palette,
            ..now.clone()
        };
        let hit = setting(tr("Palette"), "Palette", name, current, shown);
        rows.push((format!("{name} {theme}"), hit));
    }

    // `settings` in an ordinary search gets one row that leads here, not the whole list
    if in_search {
        rows.push((
            tr("settings preferences config"),
            Hit {
                title: tr("spot settings"),
                subtitle: tr("Style, colours and shortcuts"),
                kind: tr("Settings"),
                icon: themed("preferences-system-symbolic"),
                then: Then::Query(PREFIX.to_string()),
                ..Default::default()
            },
        ));
    }
    rows.push((
        tr("settings preferences config"),
        Hit {
            title: tr("Open spot.conf"),
            subtitle: config::config_path()
                .to_string_lossy()
                .replacen(home(), "~", 1),
            kind: tr("Settings"),
            ..open_config()
        },
    ));

    let keys = tr("shortcuts keys keyboard");
    let mut shortcut = |title: String, accels: String| {
        let hit = Hit {
            title,
            subtitle: if accels.is_empty() {
                tr("No key")
            } else {
                accels
            },
            kind: tr("Shortcut"),
            icon: themed("preferences-desktop-keyboard-shortcuts-symbolic"),
            verb: tr("Change in spot.conf"),
            ..open_config()
        };
        rows.push((keys.clone(), hit));
    };
    let labels = |command| {
        let mut labels: Vec<String> = config
            .keys
            .accels(command)
            .map(config::accel_label)
            .collect();
        labels.dedup(); // Return and the keypad's Enter read the same
        labels.join("  ·  ")
    };
    for (command, title) in [
        (Command::Next, tr("Next result")),
        (Command::Previous, tr("Previous result")),
        (Command::NextPage, tr("Next page")),
        (Command::PreviousPage, tr("Previous page")),
        (Command::Alternate, tr("Second action of the result")),
        (Command::Back, tr("Leave the settings, or close")),
        (Command::Settings, tr("Settings")),
    ] {
        shortcut(title, labels(command));
    }
    let picks: Vec<String> = [1, 9]
        .into_iter()
        .filter_map(|n| config.keys.label(Command::Pick(n)))
        .collect();
    shortcut(tr("Launch the result with that number"), picks.join(" … "));
    rows
}

/// One value of one setting: previewed while selected, written by Enter, and the window stays
/// open so the next setting can be changed.
fn setting(label: String, key: &'static str, name: &str, current: bool, shown: Appearance) -> Hit {
    let value = name.to_owned();
    Hit {
        title: format!("{label}: {name}"),
        kind: if current {
            tr("Current")
        } else {
            String::new()
        },
        icon: themed(if current {
            "object-select-symbolic"
        } else {
            "preferences-desktop-appearance-symbolic"
        }),
        verb: tr("Apply"),
        activate: Box::new(move |_| config::set("Appearance", key, &value)),
        preview: Some(Box::new(move || style::preview(&shown))),
        then: Then::Refresh,
        ..Default::default()
    }
}

/// Opens `spot.conf` in the user's editor, written from the commented template first when
/// there is none.
fn open_config() -> Hit {
    Hit {
        icon: themed("document-edit-symbolic"),
        verb: tr("Open spot.conf"),
        activate: Box::new(|ctx| {
            let path = config::config_path();
            config::ensure_file(&path)?;
            gio::AppInfo::launch_default_for_uri(&gio::File::for_path(&path).uri(), Some(ctx))
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(filter: &str, whole_words: bool) -> Vec<String> {
        let palettes = style::palettes(std::path::Path::new(""));
        let config = Config::parse("[Appearance]\nStyle=glass\n", &palettes);
        select(all(&config, &palettes, whole_words), filter, whole_words)
            .into_iter()
            .map(|hit| hit.title)
            .collect()
    }

    #[test]
    fn the_prefix_alone_lists_everything() {
        let all = titles("", false);
        for expected in [
            format!("{}: compact", tr("Style")),
            format!("{}: dark", tr("Colour scheme")),
            format!("{}: none", tr("Palette")),
            format!("{}: nord", tr("Palette")),
        ] {
            assert!(all.contains(&expected), "{expected}");
        }
        assert!(all.contains(&tr("Open spot.conf")));
        assert!(all.contains(&tr("Next result")));
    }

    #[test]
    fn words_after_the_prefix_narrow_the_list() {
        let palettes = style::palettes(std::path::Path::new(""));
        let appearance_rows = Style::ALL.len() + Scheme::ALL.len() + 1 + palettes.len();
        assert_eq!(titles("gla", false), [format!("{}: glass", tr("Style"))]);
        assert_eq!(titles("palette no", false).len(), 2); // none and nord
        assert_eq!(titles("zzz", false), [] as [&str; 0]);
        // a keyword finds the whole family
        assert_eq!(titles("theme", false).len(), appearance_rows);
        assert_eq!(titles("keys", false).len(), 8);
    }

    #[test]
    fn an_ordinary_query_needs_a_whole_keyword() {
        assert_eq!(titles("glass", true), [format!("{}: glass", tr("Style"))]);
        let palettes = style::palettes(std::path::Path::new(""));
        assert_eq!(
            titles("theme", true).len(),
            Style::ALL.len() + Scheme::ALL.len() + 1 + palettes.len()
        );
        assert_eq!(
            titles("settings", true),
            [tr("spot settings"), tr("Open spot.conf")]
        );
        // on the way to an application's name, nothing
        for query in ["", "the", "set", "gla", "next", "style:", "open"] {
            assert_eq!(titles(query, true), [] as [&str; 0], "{query}");
        }
    }

    #[test]
    fn the_value_in_effect_is_marked() {
        let palettes = style::palettes(std::path::Path::new(""));
        let config = Config::parse("[Appearance]\nStyle=glass\n", &palettes);
        let current: Vec<String> = all(&config, &palettes, false)
            .into_iter()
            .filter(|(_, hit)| hit.kind == tr("Current"))
            .map(|(_, hit)| hit.title)
            .collect();
        assert_eq!(
            current,
            [
                format!("{}: glass", tr("Style")),
                format!("{}: system", tr("Colour scheme")),
                format!("{}: none", tr("Palette")),
            ]
        );
    }

    #[test]
    fn settings_stay_open_and_preview() {
        let palettes = style::palettes(std::path::Path::new(""));
        let rows = all(&Config::parse("", &palettes), &palettes, true);
        let (_, style) = &rows[0];
        assert!(style.preview.is_some() && style.then == Then::Refresh);
        let leads = rows
            .iter()
            .find(|(_, hit)| hit.title == tr("spot settings"))
            .unwrap();
        assert_eq!(leads.1.then, Then::Query(">".into()));
    }
}

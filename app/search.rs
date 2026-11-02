//! Ranking, path filtering and launch counts. No GTK in here, so it is unit-tested.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::{fs, io};

pub const MAX_APPS: usize = 8;
pub const MAX_FILES: usize = 25;
pub const MAX_PROVIDER_RESULTS: usize = 5;
pub const MIN_PROVIDER_QUERY: usize = 2;
pub const MIN_FILE_QUERY: usize = 3; // 2-letter queries yield ~100k plocate candidates (~0.5 s); 3 letters ~30k

pub fn home() -> &'static str {
    static HOME: OnceLock<String> = OnceLock::new();
    HOME.get_or_init(|| std::env::var("HOME").unwrap_or_default())
}

fn is_separator(c: char) -> bool {
    " -_./".contains(c)
}

/// Character positions refer to the original title, including non-ASCII text.
#[derive(Debug, PartialEq)]
pub struct Match {
    pub score: i32,
    pub positions: Vec<usize>,
}

fn folded(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

pub fn fuzzy_match(query: &str, text: &str) -> Option<Match> {
    let q: Vec<char> = query.chars().map(folded).collect();
    let original: Vec<char> = text.chars().collect();
    let t: Vec<char> = original.iter().copied().map(folded).collect();
    let matched = |score, positions| Some(Match { score, positions });
    if q.is_empty() {
        return matched(0, vec![]);
    }
    let word_start = |i: usize| {
        i == 0
            || is_separator(t[i - 1])
            || (original[i].is_uppercase() && original[i - 1].is_lowercase())
    };
    // Bands are wider than usage's maximum bonus (100), even for long names.
    if let Some(i) = t.windows(q.len()).position(|w| w == q) {
        return matched(
            4000 + if word_start(i) { 500 } else { 0 }
                - (i.min(50) * 6 + (t.len() / 4).min(50)) as i32,
            (i..i + q.len()).collect(),
        );
    }
    let mut positions = Vec::with_capacity(q.len());
    let mut pos = 0;
    for &ch in &q {
        if let Some(offset) = t[pos..].iter().position(|&c| c == ch) {
            pos += offset;
            positions.push(pos);
            pos += 1;
        } else {
            positions.clear();
            break;
        }
    }
    if !positions.is_empty() {
        // Initials are matched independently: an earlier mid-word letter must not hide them.
        let mut initials = Vec::with_capacity(q.len());
        let mut next = 0;
        for (i, &c) in t.iter().enumerate() {
            if word_start(i) && c == q[next] {
                initials.push(i);
                next += 1;
                if next == q.len() {
                    break;
                }
            }
        }
        if initials.len() == q.len() {
            return matched(3000, initials);
        }
        return matched(2000 - (t.len().min(100)) as i32, positions);
    }
    if !(4..=128).contains(&q.len()) {
        return None;
    }
    let limit = if q.len() >= 8 { 2 } else { 1 };
    for start in (0..t.len()).filter(|&i| word_start(i)) {
        for len in q.len().saturating_sub(1)..=q.len() + 1 {
            if start + len > t.len() {
                continue;
            }
            if distance(&q, &t[start..start + len], limit) <= limit {
                let common = q
                    .iter()
                    .zip(&t[start..])
                    .take_while(|(a, b)| a == b)
                    .count();
                return matched(1000, (start..start + common).collect());
            }
        }
    }
    None
}

/// Bounded optimal-string-alignment Damerau-Levenshtein, three stack rows.
fn distance(a: &[char], b: &[char], limit: usize) -> usize {
    let mut before = [0usize; 130];
    let mut prev = [0usize; 130];
    let mut row = [0usize; 130];
    for (j, value) in prev.iter_mut().enumerate().take(b.len() + 1) {
        *value = j;
    }
    for i in 1..=a.len() {
        row[0] = i;
        for j in 1..=b.len() {
            row[j] = (prev[j] + 1)
                .min(row[j - 1] + 1)
                .min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                row[j] = row[j].min(before[j - 2] + 1);
            }
        }
        if *row[..=b.len()].iter().min().unwrap() > limit {
            return limit + 1;
        }
        before = prev;
        prev = row;
    }
    prev[b.len()]
}

pub fn fuzzy_score(query: &str, text: &str) -> i32 {
    fuzzy_match(query, text).map_or(-1, |m| m.score)
}

/// Name first, then generic name, then keywords / executable, each less direct than the last.
pub fn app_score(query: &str, name: &str, generic: Option<&str>, aliases: &[String]) -> i32 {
    std::iter::once(fuzzy_score(query, name))
        .chain(generic.filter(|g| !g.is_empty()).map(|g| {
            let s = fuzzy_score(query, g);
            if s >= 0 { s - 40 } else { -1 }
        }))
        .chain(aliases.iter().map(|a| {
            let s = fuzzy_score(query, a);
            if s >= 0 { s - 60 } else { -1 }
        }))
        .max()
        .unwrap_or(-1)
}

/// Keep paths inside $HOME, skipping hidden entries and node_modules.
pub fn is_wanted_path(path: &str) -> bool {
    let Some(rel) = path.strip_prefix(home()).filter(|r| r.starts_with('/')) else {
        return false;
    };
    !rel.contains("/.") && !format!("{rel}/").contains("/node_modules/")
}

/// Launch counts per application, so what you use floats up. Persisted as JSON.
pub struct Usage {
    path: PathBuf,
    counts: HashMap<String, u32>,
}

impl Usage {
    pub fn new(path: PathBuf) -> Self {
        let counts = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Usage { path, counts }
    }

    pub fn bump(&mut self, key: &str) -> io::Result<()> {
        *self.counts.entry(key.to_owned()).or_insert(0) += 1;
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string(&self.counts)?)?;
        fs::rename(tmp, &self.path) // atomic: never a half-written file
    }

    pub fn bonus(&self, key: &str) -> i32 {
        // ponytail: plain counts, add time decay if old habits stick around too long
        10 * self.counts.get(key).copied().unwrap_or(0).min(10) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fuzzy() {
        assert!(fuzzy_score("fire", "Firefox") > fuzzy_score("fire", "LibreOffice Firebird"));
        assert!(fuzzy_score("ff", "Firefox") > 0); // subsequence match
        assert!(fuzzy_score("ff", "Firefox") < fuzzy_score("fire", "Firefox")); // literal beats subsequence
        assert_eq!(fuzzy_score("xyz", "Firefox"), -1);
        // word start bonus
        assert!(fuzzy_score("term", "GNOME Terminal") > fuzzy_score("term", "Determinant"));
    }

    #[test]
    fn app() {
        assert!(app_score("browser", "Firefox", Some("Web Browser"), &[]) >= 0); // generic name
        assert!(
            app_score(
                "gimp",
                "GNU Image Manipulation Program",
                None,
                &aliases(&["GIMP"])
            ) >= 0
        ); // keyword
        assert!(app_score("nvim", "Neovim", None, &aliases(&["nvim"])) >= 0); // executable
        assert_eq!(
            app_score("zzz", "Neovim", Some("Editor"), &aliases(&["nvim"])),
            -1
        );
        let name = app_score("fire", "Firefox", Some("Web Browser"), &aliases(&["fire"]));
        let generic = app_score("fire", "Other", Some("Firewall"), &aliases(&["fire"]));
        let alias = app_score("fire", "Other", Some("Tool"), &aliases(&["fire"]));
        assert!(name > generic && generic > alias); // directness order
    }

    #[test]
    fn wanted_path() {
        let home = home();
        assert!(is_wanted_path(&format!("{home}/Documents/notes.md")));
        assert!(is_wanted_path(&format!("{home}/projects/spot/main.rs")));
        assert!(!is_wanted_path(&format!("{home}/.cache/thing"))); // hidden dir
        assert!(!is_wanted_path(&format!("{home}/projects/.env"))); // hidden file
        assert!(!is_wanted_path(&format!("{home}/app/node_modules/x.js")));
        assert!(!is_wanted_path(&format!("{home}/app/node_modules")));
        assert!(!is_wanted_path("/etc/hosts"));
        assert!(!is_wanted_path(&format!("{home}2/file"))); // sibling home, same prefix
        assert!(!is_wanted_path(home));
    }

    #[test]
    fn usage_counts_persist() {
        let dir = std::env::temp_dir().join(format!("spot-test-{}", std::process::id()));
        let path = dir.join("sub/usage.json");
        let mut usage = Usage::new(path.clone());
        assert_eq!(usage.bonus("a"), 0);
        for _ in 0..3 {
            usage.bump("a").unwrap();
        }
        assert_eq!(Usage::new(path).bonus("a"), 30); // reloaded from disk
        for _ in 0..20 {
            usage.bump("a").unwrap();
        }
        assert_eq!(usage.bonus("a"), 100); // capped
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod matching_tests {
    use super::*;
    #[test]
    fn tiers_and_positions() {
        assert_eq!(
            fuzzy_match("vsc", "Visual Studio Code").unwrap().positions,
            [0, 7, 14]
        );
        assert!(fuzzy_score("vsc", "Visual Studio Code") > fuzzy_score("vsc", "Vasecraft"));
        assert!(fuzzy_score("gt", "GNOME Terminal") >= 3000);
        for (q, t) in [
            ("chorme", "Chrome"),
            ("chorme", "Google Chrome"),
            ("firfox", "Firefox"),
            ("calandar", "Calendar"),
        ] {
            assert!(fuzzy_score(q, t) >= 0, "{q}: {t}");
        }
        assert_eq!(fuzzy_score("ab", "Acorn"), -1);
        assert_eq!(fuzzy_score("xyz", "Xylophone"), -1);
        assert!(fuzzy_score("vsc", "avsc") > fuzzy_score("vsc", "Visual Studio Code") + 100);
        assert_eq!(fuzzy_match("ét", "Été").unwrap().positions, [0, 1]);
        assert_eq!(fuzzy_match("chorme", "Chrome").unwrap().positions, [0, 1]);
        assert!(fuzzy_score("aB", "alphaBeta") >= 3000);
    }
}

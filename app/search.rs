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

/// Score a subsequence match. Returns -1 when there is no match.
pub fn fuzzy_score(query: &str, text: &str) -> i32 {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    if q.is_empty() {
        return 0;
    }
    if let Some(idx) = t.windows(q.len()).position(|w| w == q) {
        // literal match always wins; a word start beats a mid-word hit
        let word_start = idx == 0 || is_separator(t[idx - 1]);
        return 1000 - idx as i32 * 6 - (t.len() / 4) as i32 + if word_start { 30 } else { 0 };
    }
    let (mut score, mut pos, mut prev) = (0i32, 0usize, -2i64);
    for ch in q {
        let Some(offset) = t[pos..].iter().position(|&c| c == ch) else {
            return -1;
        };
        pos += offset;
        score += if pos as i64 == prev + 1 { 12 } else { 1 };
        if pos == 0 || is_separator(t[pos - 1]) {
            score += 9;
        }
        prev = pos as i64;
        pos += 1;
    }
    score - (t.len() / 12) as i32
}

/// Name first, then generic name, then keywords / executable, each less direct than the last.
pub fn app_score(query: &str, name: &str, generic: Option<&str>, aliases: &[String]) -> i32 {
    let score = fuzzy_score(query, name);
    if score >= 0 {
        return score;
    }
    if let Some(generic) = generic.filter(|g| !g.is_empty()) {
        let score = fuzzy_score(query, generic);
        if score >= 0 {
            return score - 40;
        }
    }
    let best = aliases
        .iter()
        .map(|a| fuzzy_score(query, a))
        .filter(|&s| s >= 0)
        .max();
    best.map_or(-1, |s| s - 60)
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

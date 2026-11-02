//! Privacy gates, OpenSearch decoding and a bounded, memory-only prefix cache.
use std::collections::VecDeque;

pub const ENGINES: &[&str] = &["duckduckgo", "google", "startpage", "brave"];
pub fn endpoint(engine: &str) -> Option<String> {
    let url = match engine {
        "duckduckgo" => "https://duckduckgo.com/ac/?q=%s&type=list",
        "google" => "https://suggestqueries.google.com/complete/search?client=firefox&q=%s",
        "startpage" => "https://www.startpage.com/suggestions?q=%s&format=opensearch",
        "brave" => "https://search.brave.com/api/suggest?q=%s",
        custom => custom,
    };
    let uri = gio::glib::Uri::parse(
        &url.replace("%s", "spotquery"),
        gio::glib::UriFlags::ENCODED,
    )
    .ok()?;
    (uri.scheme() == "https"
        && uri.host().is_some()
        && uri.userinfo().is_none()
        && uri.fragment().is_none()
        && url.matches("%s").count() == 1)
        .then(|| url.to_owned())
}

pub fn eligible(query: &str) -> bool {
    let length = query.chars().count();
    (2..=100).contains(&length)
        && !query
            .chars()
            .any(|c| c.is_control() || ". /\\@:\t!>~".contains(c) && c != ' ')
        && !query.to_ascii_lowercase().contains("localhost")
        && !query.starts_with(['-', '$'])
        && crate::calc::eval(query).is_none()
}

pub fn decode(body: &str, query: &str) -> Vec<String> {
    if body.len() > 65536 {
        return vec![];
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return vec![];
    };
    if value.get(0).and_then(|v| v.as_str()) != Some(query) {
        return vec![];
    }
    let Some(values) = value.get(1).and_then(|v| v.as_array()) else {
        return vec![];
    };
    let mut result = vec![];
    for value in values {
        if let Some(s) = value
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control))
            && !result.iter().any(|r| r == s)
        {
            result.push(s.to_owned());
            if result.len() == 4 {
                break;
            }
        }
    }
    result
}

#[derive(Default)]
pub struct Cache(VecDeque<(String, String, Vec<String>)>);
impl Cache {
    pub fn get(&self, engine: &str, query: &str) -> Option<Vec<String>> {
        self.0
            .iter()
            .rev()
            .filter(|(e, q, _)| e == engine && query.starts_with(q))
            .find_map(|(_, q, values)| {
                let matching: Vec<_> = values
                    .iter()
                    .filter(|s| s.to_lowercase().starts_with(&query.to_lowercase()))
                    .cloned()
                    .collect();
                (q == query || !matching.is_empty()).then_some(if q == query {
                    values.clone()
                } else {
                    matching
                })
            })
    }
    pub fn put(&mut self, engine: String, query: String, values: Vec<String>) {
        self.0.retain(|(e, q, _)| *e != engine || *q != query);
        if self.0.len() == 64 {
            self.0.pop_front();
        }
        self.0.push_back((engine, query, values));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn privacy_gate() {
        for query in [
            "github.com",
            "moi@exemple.fr",
            "~/docs",
            "a/b",
            "localhost 3000",
            "c:\\x",
            "a",
            "a\tb",
            "!ls",
            ">theme",
            "search: cat",
            "2+2",
            "sqrt(2)",
            "2^",
            "https://x",
            "$HOME",
        ] {
            assert!(!eligible(query), "{query}");
        }
        assert!(!eligible(&"x".repeat(101)));
        assert!(eligible("liquid gl"));
    }
    #[test]
    fn json_is_bounded_and_validated() {
        assert_eq!(
            decode(r#"["q",["one","two","one",3,"three","four","five"]]"#, "q"),
            ["one", "two", "three", "four"]
        );
        for input in ["{}", "null", "[", r#"["other",["x"]]"#, r#"["q",null]"#] {
            assert!(decode(input, "q").is_empty());
        }
    }
    #[test]
    fn prefix_cache_and_eviction() {
        let mut c = Cache::default();
        c.put("a".into(), "liquid gl".into(), vec!["liquid glass".into()]);
        assert_eq!(c.get("a", "liquid gla"), Some(vec!["liquid glass".into()]));
        assert_eq!(c.get("b", "liquid gla"), None);
        assert_eq!(c.get("a", "liquid glitter"), None);
        for i in 0..64 {
            c.put("a".into(), i.to_string(), vec![]);
        }
        assert_eq!(c.0.len(), 64);
        assert_eq!(c.get("a", "liquid gl"), None);
    }
    #[test]
    fn validates_endpoints() {
        for engine in ENGINES {
            assert!(endpoint(engine).is_some(), "{engine}");
        }
        for bad in [
            "googlex",
            "http://x/%s",
            "https://a:b@x/%s",
            "https://x",
            "https://x/%s/%s",
        ] {
            assert!(endpoint(bad).is_none(), "{bad}");
        }
        assert!(endpoint("https://example.com/?q=%s").is_some());
    }
}

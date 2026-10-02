//! GNOME Shell search providers, queried over D-Bus like the Activities overview does,
//! and the settings that say which of them to ask.

use crate::results::{Hit, file_result};
use crate::search::MAX_PROVIDER_RESULTS;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::path::Path;
use std::rc::Rc;

const IFACE: &str = "org.gnome.Shell.SearchProvider2";
const GROUP: &str = "Shell Search Provider";
const TIMEOUT_MS: i32 = 3000; // a provider may have to be D-Bus activated first

/// Called with the provider's desktop id and its hits.
pub type Callback = Rc<dyn Fn(&str, Vec<Hit>)>;

pub struct SearchProvider {
    pub desktop_id: String,
    bus_name: String,
    object_path: String,
    default_disabled: bool,
    bus: gio::DBusConnection,
    label: String,
    icon: gio::Icon,
}

impl SearchProvider {
    fn new(keyfile: &glib::KeyFile, bus: &gio::DBusConnection) -> Result<Self, glib::Error> {
        let desktop_id = keyfile.string(GROUP, "DesktopId")?.to_string();
        let info = gio_unix::DesktopAppInfo::new(&desktop_id);
        Ok(SearchProvider {
            bus_name: keyfile.string(GROUP, "BusName")?.to_string(),
            object_path: keyfile.string(GROUP, "ObjectPath")?.to_string(),
            default_disabled: keyfile.boolean(GROUP, "DefaultDisabled").unwrap_or(false),
            bus: bus.clone(),
            label: info
                .as_ref()
                .map_or(desktop_id.clone(), |i| i.display_name().to_string()),
            icon: info
                .and_then(|i| i.icon())
                .unwrap_or_else(|| gio::ThemedIcon::new("system-search-symbolic").upcast()),
            desktop_id,
        })
    }

    fn call(
        &self,
        method: &str,
        args: glib::Variant,
        reply_type: Option<&str>,
        cancellable: Option<&gio::Cancellable>,
        callback: impl FnOnce(Result<glib::Variant, glib::Error>) + 'static,
    ) {
        self.bus.call(
            Some(&self.bus_name),
            &self.object_path,
            IFACE,
            method,
            Some(&args),
            reply_type.map(|t| glib::VariantTy::new(t).unwrap()),
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            cancellable,
            callback,
        );
    }

    /// Calls `callback` once with the hits; silently nothing on error or cancellation.
    pub fn search(
        self: &Rc<Self>,
        terms: Vec<String>,
        cancellable: &gio::Cancellable,
        callback: Callback,
    ) {
        let (provider, cancel) = (self.clone(), cancellable.clone());
        self.call(
            "GetInitialResultSet",
            (terms.clone(),).to_variant(),
            Some("(as)"),
            Some(cancellable),
            move |reply| {
                let Some((mut ids,)) = reply.ok().and_then(|v| v.get::<(Vec<String>,)>()) else {
                    return;
                };
                ids.truncate(MAX_PROVIDER_RESULTS);
                if ids.is_empty() {
                    return callback(&provider.desktop_id, vec![]);
                }
                let p = provider.clone();
                provider.call(
                    "GetResultMetas",
                    (ids,).to_variant(),
                    Some("(aa{sv})"),
                    Some(&cancel),
                    move |reply| {
                        if let Ok(reply) = reply {
                            callback(&p.desktop_id, p.hits(&reply.child_value(0), &terms));
                        }
                    },
                );
            },
        );
    }

    fn hits(self: &Rc<Self>, metas: &glib::Variant, terms: &[String]) -> Vec<Hit> {
        let mut hits = vec![];
        for meta in metas.iter() {
            let dict = glib::VariantDict::new(Some(&meta));
            let text = |key: &str| {
                dict.lookup_value(key, Some(glib::VariantTy::STRING))
                    .and_then(|v| v.str().map(String::from))
                    .unwrap_or_default()
            };
            let id = text("id");
            if id.starts_with("file://") {
                // e.g. Nautilus: show it like our own file hits
                let path = gio::File::for_uri(&id).path();
                if let Some(hit) = path.and_then(|p| file_result(p.to_str()?, 0)) {
                    hits.push(hit);
                }
                continue;
            }
            let clipboard_text = text("clipboardText");
            let (provider, terms) = (self.clone(), terms.to_vec());
            let result_id = id.clone();
            hits.push(Hit {
                score: 0,
                title: text("name"),
                subtitle: text("description"),
                kind: self.label.clone(),
                icon: meta_icon(&dict).unwrap_or_else(|| self.icon.clone()),
                activate: Box::new(move |_| {
                    if !clipboard_text.is_empty() {
                        // as the Shell does: copy the result, then tell the provider
                        if let Some(display) = gdk::Display::default() {
                            display.clipboard().set_text(&clipboard_text);
                        }
                    }
                    let args = (result_id.clone(), terms.clone(), 0u32).to_variant();
                    provider.call("ActivateResult", args, None, None, |_| {});
                    Ok(())
                }),
                alt: None,
                path: None,
            });
        }
        hits
    }
}

fn meta_icon(dict: &glib::VariantDict) -> Option<gio::Icon> {
    if let Some(icon) = dict.lookup_value("icon", None) {
        return gio::Icon::deserialize(&icon); // serialized GIcon
    }
    let name = dict.lookup_value("gicon", Some(glib::VariantTy::STRING))?;
    gio::Icon::for_string(name.str()?).ok()
}

/// The providers the Activities overview would query, honouring Settings → Search.
pub fn load(bus: Option<gio::DBusConnection>) -> Vec<Rc<SearchProvider>> {
    let Some(bus) = bus else { return vec![] };
    let (mut disabled, mut enabled, mut order): (Vec<String>, Vec<String>, Vec<String>) =
        Default::default();
    const SCHEMA: &str = "org.gnome.desktop.search-providers";
    if gio::SettingsSchemaSource::default().is_some_and(|s| s.lookup(SCHEMA, true).is_some()) {
        let settings = gio::Settings::new(SCHEMA);
        if settings.boolean("disable-external") {
            return vec![];
        }
        let strv = |key| settings.strv(key).iter().map(|s| s.to_string()).collect();
        (disabled, enabled, order) = (strv("disabled"), strv("enabled"), strv("sort-order"));
    }

    let mut providers: Vec<Rc<SearchProvider>> = vec![];
    for data_dir in glib::system_data_dirs() {
        let directory = data_dir.join("gnome-shell").join("search-providers");
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        let mut names: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        names.sort();
        for path in names
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "ini"))
        {
            let keyfile = glib::KeyFile::new();
            let Ok(provider) = load_one(&keyfile, path, &bus) else {
                continue; // malformed .ini
            };
            let id = provider.desktop_id.as_str();
            if providers.iter().any(|p| p.desktop_id == id) || disabled.iter().any(|d| d == id) {
                continue;
            }
            if provider.default_disabled && !enabled.iter().any(|e| e == id) {
                continue;
            }
            providers.push(Rc::new(provider));
        }
    }
    let rank = |p: &SearchProvider| {
        order
            .iter()
            .position(|o| *o == p.desktop_id)
            .unwrap_or(order.len())
    };
    providers.sort_by(|a, b| (rank(a), &a.label).cmp(&(rank(b), &b.label)));
    providers
}

fn load_one(
    keyfile: &glib::KeyFile,
    path: &Path,
    bus: &gio::DBusConnection,
) -> Result<SearchProvider, glib::Error> {
    keyfile.load_from_file(path, glib::KeyFileFlags::NONE)?;
    SearchProvider::new(keyfile, bus)
}

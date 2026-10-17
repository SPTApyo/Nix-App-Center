//! GNOME Shell search provider: lists apps from the local AppStream
//! database while the user types in the overview.

use std::collections::HashMap;
use std::sync::OnceLock;

use gtk::gio;
use gtk::glib::{self, prelude::ToVariant};
use gtk::prelude::*;

use crate::app::AppMsg;

const PATH: &str = "/io/github/sptapyo/NixAppCenter/SearchProvider";
const XML: &str = r#"<node>
  <interface name="org.gnome.Shell.SearchProvider2">
    <method name="GetInitialResultSet">
      <arg type="as" name="terms" direction="in"/>
      <arg type="as" name="results" direction="out"/>
    </method>
    <method name="GetSubsearchResultSet">
      <arg type="as" name="previous_results" direction="in"/>
      <arg type="as" name="terms" direction="in"/>
      <arg type="as" name="results" direction="out"/>
    </method>
    <method name="GetResultMetas">
      <arg type="as" name="identifiers" direction="in"/>
      <arg type="aa{sv}" name="metas" direction="out"/>
    </method>
    <method name="ActivateResult">
      <arg type="s" name="identifier" direction="in"/>
      <arg type="as" name="terms" direction="in"/>
      <arg type="u" name="timestamp" direction="in"/>
    </method>
    <method name="LaunchSearch">
      <arg type="as" name="terms" direction="in"/>
      <arg type="u" name="timestamp" direction="in"/>
    </method>
  </interface>
</node>"#;

/// Where activated results go, once the window exists.
pub static APP_SENDER: OnceLock<relm4::Sender<AppMsg>> = OnceLock::new();

/// Exports the provider on the app's bus. Call from `startup`.
pub fn register(app: &gtk::Application) {
    let Some(conn) = app.dbus_connection() else {
        return;
    };
    let Some(iface) = gio::DBusNodeInfo::for_xml(XML)
        .ok()
        .and_then(|n| n.lookup_interface("org.gnome.Shell.SearchProvider2"))
    else {
        return;
    };
    nix_app_center_core::init_appstream_db();
    let app = app.clone();
    let _ = conn
        .register_object(PATH, &iface)
        .method_call(move |_, _, _, _, method, params, invocation| {
            let reply = handle(&app, method, &params);
            invocation.return_value(reply.as_ref());
        })
        .build();
}

fn handle(app: &gtk::Application, method: &str, params: &glib::Variant) -> Option<glib::Variant> {
    match method {
        "GetInitialResultSet" => {
            let (terms,) = params.get::<(Vec<String>,)>()?;
            Some((ids(&terms),).to_variant())
        }
        "GetSubsearchResultSet" => {
            let (_, terms) = params.get::<(Vec<String>, Vec<String>)>()?;
            Some((ids(&terms),).to_variant())
        }
        "GetResultMetas" => {
            let (ids,) = params.get::<(Vec<String>,)>()?;
            let metas: Vec<HashMap<String, glib::Variant>> =
                ids.iter().map(|id| meta(id)).collect();
            Some((metas,).to_variant())
        }
        "ActivateResult" => {
            let (id, _, _) = params.get::<(String, Vec<String>, u32)>()?;
            app.activate();
            let name = name_of(&id);
            std::thread::spawn(move || {
                if let (Some(pkg), Some(tx)) = (
                    nix_app_center_core::package_for_app(&id, &name),
                    APP_SENDER.get(),
                ) {
                    let _ = tx.send(AppMsg::OpenDetail(pkg));
                }
            });
            None
        }
        "LaunchSearch" => {
            let (terms, _) = params.get::<(Vec<String>, u32)>()?;
            app.activate();
            if let Some(tx) = APP_SENDER.get() {
                let _ = tx.send(AppMsg::Search(terms.join(" ")));
            }
            None
        }
        _ => None,
    }
}

fn ids(terms: &[String]) -> Vec<String> {
    nix_app_center_core::search_local(terms, 8)
        .into_iter()
        .map(|h| h.app_id)
        .collect()
}

fn name_of(app_id: &str) -> String {
    nix_app_center_core::local_app(app_id)
        .map(|h| h.name)
        .unwrap_or_default()
}

fn meta(app_id: &str) -> HashMap<String, glib::Variant> {
    let (name, summary) = nix_app_center_core::local_app(app_id)
        .map(|h| (h.name, h.summary))
        .unwrap_or_default();
    // The app's icon, else a generic one.
    let icon = gio::ThemedIcon::from_names(&[app_id, "package-x-generic"]);
    let mut m = HashMap::new();
    m.insert("id".to_string(), app_id.to_variant());
    m.insert("name".to_string(), name.to_variant());
    m.insert("description".to_string(), summary.to_variant());
    if let Some(v) = icon.serialize() {
        m.insert("icon".to_string(), v);
    }
    m
}

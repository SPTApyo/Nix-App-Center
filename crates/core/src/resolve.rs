//! Le « DNS des apps » — résout un attribut nixpkgs vers un app-id AppStream/Flathub.
//!
//! nixpkgs et Flathub sont deux univers sans correspondance officielle. Le dataset auto
//! (façon `nixos-appstream-data`) couvre ~825 attrs mais laisse des trous béants (Firefox,
//! toutes les apps *unfree*) et se trompe parfois (`gimp` → un addon plutôt que
//! `org.gimp.GIMP`). Cette couche **curée** comble et corrige, par ordre de priorité :
//!
//!   1. overlay **utilisateur** (`data_dir/app-mapping.json`) — corrections locales,
//!      backend du bouton « Signaler un problème » ;
//!   2. overlay **communautaire** (embarqué, curé dans le repo par PR).
//!
//! Si aucun ne connaît l'attr, la résolution rend `None` et l'appelant retombe sur le
//! dataset auto puis l'heuristique historique. On ne mélange jamais identité (stable,
//! ici) et contenu (icône/avis, récupérés en direct ailleurs à partir de l'app-id résolu).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Overlay curé dans le dépôt, embarqué dans le binaire.
const COMMUNITY_OVERLAY: &str = include_str!("data/app_mapping.json");

/// Format commun aux deux overlays. `note` documente le fichier (communautaire) ; les
/// entrées vivent dans `mappings`. Un enrobage (plutôt qu'une map nue) laisse la place à
/// des champs futurs (confiance, source d'un signalement) sans casser le format.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Overlay {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(default)]
    mappings: HashMap<String, String>,
}

/// L'overlay communautaire, parsé une seule fois (indépendant de l'environnement).
fn community() -> &'static HashMap<String, String> {
    static M: OnceLock<HashMap<String, String>> = OnceLock::new();
    M.get_or_init(|| {
        serde_json::from_str::<Overlay>(COMMUNITY_OVERLAY)
            .map(|o| o.mappings)
            .unwrap_or_default()
    })
}

/// Overlay local modifiable (à côté de `kept.json`/`tried.json`).
pub(crate) fn user_overlay_path() -> PathBuf {
    crate::paths::data_dir().join("app-mapping.json")
}

fn user_overlay() -> Overlay {
    std::fs::read(user_overlay_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Résout `attr` → app-id via les overlays curés. `None` si inconnu — l'appelant retombe
/// alors sur le dataset auto / l'heuristique.
pub fn resolve_app_id(attr: &str) -> Option<String> {
    let mut user = user_overlay();
    if let Some(id) = user.mappings.remove(attr) {
        return Some(id); // la correction locale prime toujours
    }
    community().get(attr).cloned()
}

/// Enregistre une correction/signalement local. **Placeholder** du bouton « Signaler un
/// problème » : pour l'instant on écrit dans l'overlay utilisateur (pris en compte à la
/// résolution suivante) ; l'envoi vers un dépôt communautaire partagé viendra plus tard.
pub fn report_mapping(attr: &str, app_id: &str) -> std::io::Result<()> {
    let mut o = user_overlay();
    o.mappings.insert(attr.to_string(), app_id.to_string());
    let json = serde_json::to_vec_pretty(&o).map_err(std::io::Error::other)?;
    crate::paths::write_atomic(&user_overlay_path(), &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn community_overlay_covers_known_gaps() {
        let _g = crate::env_lock();
        let dir = std::env::temp_dir().join(format!("onyx-resolve-c-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_DATA_HOME", &dir); // overlay utilisateur vide → tombe sur le communautaire

        assert_eq!(resolve_app_id("firefox").as_deref(), Some("org.mozilla.firefox"));
        assert_eq!(resolve_app_id("discord").as_deref(), Some("com.discordapp.Discord"));
        // Désambiguïsation : gimp doit rendre l'app-id, pas un addon.
        assert_eq!(resolve_app_id("gimp").as_deref(), Some("org.gimp.GIMP"));
        assert_eq!(resolve_app_id("attr-inexistant-xyz"), None);

        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn user_overlay_takes_precedence_and_report_persists() {
        let _g = crate::env_lock();
        let dir = std::env::temp_dir().join(format!("onyx-resolve-u-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_DATA_HOME", &dir);

        assert_eq!(resolve_app_id("mon-app-perso"), None);

        // L'utilisateur signale une correspondance : elle persiste et se résout.
        report_mapping("mon-app-perso", "com.example.MyApp").unwrap();
        assert_eq!(resolve_app_id("mon-app-perso").as_deref(), Some("com.example.MyApp"));

        // Et elle prime sur le communautaire.
        report_mapping("firefox", "org.mozilla.firefox.custom").unwrap();
        assert_eq!(resolve_app_id("firefox").as_deref(), Some("org.mozilla.firefox.custom"));

        std::env::remove_var("XDG_DATA_HOME");
    }
}

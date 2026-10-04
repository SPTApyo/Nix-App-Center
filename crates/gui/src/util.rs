//! Helpers partagés par les vues et composants (icônes, textures, init de ligne).

use onyx_core::Package;

/// Affiche un toast simple sur un `AdwToastOverlay`.
pub fn toast(overlay: &adw::ToastOverlay, text: &str) {
    overlay.add_toast(adw::Toast::new(text));
}

/// Met `text` dans le presse-papier. Le core n'y touche jamais (il ignore GTK) : c'est
/// à la façade de décider quoi faire quand une action ne peut pas aboutir seule.
pub fn copy_to_clipboard(text: &str) {
    if let Some(display) = gtk::gdk::Display::default() {
        display.clipboard().set_text(text);
    }
}

/// Traduit le résultat d'une application de config en message affichable, et effectue
/// l'effet de bord qui incombe à la façade (copier la commande quand Onyx n'a pas pu
/// l'exécuter lui-même). Partagé par la page détail et la vue des mises à jour.
pub fn describe_apply(res: Result<onyx_core::ApplyOutcome, String>) -> String {
    use onyx_core::ApplyOutcome::*;
    match res {
        Ok(Done(line)) if line.is_empty() => "Config appliquée.".to_string(),
        Ok(Done(line)) => format!("Config appliquée : {line}"),
        Ok(Launched) => "Application lancée dans un terminal — suis le build là-bas.".to_string(),
        Ok(NoTerminal(argv)) => {
            copy_to_clipboard(&argv.join(" "));
            "Aucun terminal trouvé — commande d'application copiée.".to_string()
        }
        Ok(UnknownLayout) => {
            "Onyx ne trouve pas ta config Nix : rien n'a été appliqué.".to_string()
        }
        Err(e) => format!("Échec : {e}"),
    }
}

/// Décode des octets d'image (PNG/JPEG…) en texture GTK. À appeler sur le thread
/// principal (création de Texture). `None` si décodage impossible.
pub fn texture_from_bytes(data: &[u8]) -> Option<gtk::gdk::Texture> {
    let pixbuf =
        gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(data.to_vec())).ok()?;
    Some(gtk::gdk::Texture::for_pixbuf(&pixbuf))
}

/// Idem mais redimensionne à `px`×`px`. Indispensable pour les icônes des tuiles :
/// un `gtk::Picture` réclame la taille intrinsèque de l'image (128/256px), ce qui
/// élargirait les tuiles (et, en FlowBox homogeneous, toutes les tuiles) → on
/// borne donc la texture à la taille voulue.
pub fn texture_from_bytes_sized(data: &[u8], px: i32) -> Option<gtk::gdk::Texture> {
    let pixbuf =
        gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(data.to_vec())).ok()?;
    let scaled = pixbuf
        .scale_simple(px, px, gtk::gdk_pixbuf::InterpType::Bilinear)
        .unwrap_or(pixbuf);
    Some(gtk::gdk::Texture::for_pixbuf(&scaled))
}

use adw::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

// ── Scroll molette fluide ─────────────────────────────────────────────────────

/// GTK4 scrolle par **crans discrets** à la molette (aucune animation), ce qui
/// est visuellement désagréable. On intercepte les events molette et on anime
/// l'ajustement vertical avec une `SpringAnimation` libadwaita → scroll inertiel
/// lisse. Le pavé tactile / écran tactile (déjà fluides en natif) sont laissés
/// au comportement par défaut.
pub fn smooth_scroll(scroller: &gtk::ScrolledWindow) {
    let vadj = scroller.vadjustment();

    // Cible d'animation : pousser la valeur dans l'ajustement à chaque frame.
    let cb = adw::CallbackAnimationTarget::new({
        let vadj = vadj.clone();
        move |v| vadj.set_value(v)
    });
    // damping_ratio=1.0 (critiquement amorti, zéro rebond), mass=1, stiffness=400.
    let params = adw::SpringParams::new(1.0, 1.0, 400.0);
    let anim = adw::SpringAnimation::new(scroller, 0.0, 0.0, params, cb);
    anim.set_clamp(true); // jamais d'overshoot au-delà des bornes
    anim.set_epsilon(0.1);

    let target = Rc::new(Cell::new(0f64));
    let ctrl = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    ctrl.connect_scroll(move |c, _dx, dy| {
        // Laisse le pavé/écran tactile au scroll natif (déjà lisse).
        if let Some(ev) = c.current_event() {
            if let Some(dev) = ev.device() {
                if dev.source() != gtk::gdk::InputSource::Mouse {
                    return gtk::glib::Propagation::Proceed;
                }
            }
        }

        let page = vadj.page_size();
        let max = (vadj.upper() - page).max(vadj.lower());
        // Si une animation tourne, on enchaîne depuis sa cible (les crans
        // s'additionnent) ; sinon on part de la position réelle.
        let playing = anim.state() == adw::AnimationState::Playing;
        let base = if playing { target.get() } else { vadj.value() };
        let step = (page * 0.16).clamp(48.0, 180.0); // ~ quelques lignes par cran
        let to = (base + dy * step).clamp(vadj.lower(), max);

        target.set(to);
        anim.set_value_from(vadj.value());
        anim.set_value_to(to);
        anim.play();
        gtk::glib::Propagation::Stop
    });
    scroller.add_controller(ctrl);
}

// ── Skeletons (placeholders animés pendant le chargement) ─────────────────────

fn skeleton_box(w: i32, h: i32) -> gtk::Box {
    let b = gtk::Box::builder().width_request(w).height_request(h).build();
    b.add_css_class("skeleton");
    b
}

/// Contenu d'une ligne skeleton, calqué EXACTEMENT sur `PackageRow`
/// (icône 64, marges/espacements identiques) → aucun saut de layout.
fn skeleton_row() -> gtk::Box {
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .spacing(6)
        .hexpand(true)
        .build();
    text.append(&skeleton_box(180, 15));
    text.append(&skeleton_box(260, 12));

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();
    row.append(&skeleton_box(64, 64));
    row.append(&text);
    row
}

/// Une tuile skeleton, calquée EXACTEMENT sur `AppTile`
/// (carte, width 190, padding 17, gap 14, icône 64).
pub fn skeleton_tile() -> gtk::Box {
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .spacing(6)
        .build();
    text.append(&skeleton_box(90, 16));
    text.append(&skeleton_box(120, 13));
    text.append(&skeleton_box(80, 13));

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .margin_top(17)
        .margin_bottom(17)
        .margin_start(17)
        .margin_end(17)
        .build();
    inner.append(&skeleton_box(64, 64));
    inner.append(&text);

    let tile = gtk::Box::builder()
        .width_request(190)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Start)
        .build();
    tile.add_css_class("card");
    tile.append(&inner);
    tile
}

/// Liste skeleton dans une `boxed-list` (même conteneur que les vraies listes).
pub fn skeleton_list(n: usize) -> gtk::ListBox {
    let lb = gtk::ListBox::new();
    lb.add_css_class("boxed-list");
    lb.set_selection_mode(gtk::SelectionMode::None);
    lb.set_valign(gtk::Align::Start);
    for _ in 0..n {
        lb.append(&skeleton_row());
    }
    lb
}

/// Grille skeleton, même config de FlowBox que les vraies grilles (3 colonnes).
pub fn skeleton_grid(n: usize) -> gtk::FlowBox {
    let f = gtk::FlowBox::builder()
        .homogeneous(true)
        .column_spacing(14)
        .row_spacing(14)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .selection_mode(gtk::SelectionMode::None)
        .hexpand(true)
        .valign(gtk::Align::Start)
        .build();
    for _ in 0..n {
        f.append(&skeleton_tile());
    }
    f
}

/// État d'affichage d'une icône (placeholder shimmer → icône réelle / générique).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IconState {
    /// Téléchargement en cours → on montre le shimmer animé.
    Loading,
    /// Pas d'icône AppStream → icône générique du thème.
    Generic,
    /// Icône réelle chargée.
    Real,
}

/// Métriques de l'écran principal : `(largeur_physique_px, facteur_d_échelle)`.
/// Largeur physique = largeur logique × échelle. Repli `(1920, 1)` si pas d'écran.
pub(crate) fn screen_metrics() -> (i32, i32) {
    if let Some(display) = gtk::gdk::Display::default() {
        if let Some(obj) = display.monitors().item(0) {
            if let Ok(m) = obj.downcast::<gtk::gdk::Monitor>() {
                let scale = m.scale_factor().max(1);
                let w = m.geometry().width().max(1);
                return (w * scale, scale);
            }
        }
    }
    (1920, 1)
}

/// Échappe le texte destiné aux widgets qui interprètent le markup Pango
/// (`adw::ActionRow` titre/sous-titre) — sinon un `&` casse le parsing.
pub fn esc(s: &str) -> String {
    gtk::glib::markup_escape_text(s).to_string()
}

/// Une rangée de 5 étoiles (calque de `GsStarWidget`) : pleines jusqu'à la
/// moyenne arrondie, vides ensuite.
pub fn stars_widget(stars: f32, size: i32) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    b.add_css_class("app-stars");
    let filled = stars.round().clamp(0.0, 5.0) as i32;
    for i in 0..5 {
        let img = gtk::Image::from_icon_name("starred-symbolic");
        img.set_pixel_size(size);
        img.add_css_class(if i < filled { "star-on" } else { "star-off" });
        b.append(&img);
    }
    b
}

/// Date relative en français (« Aujourd'hui », « Il y a 2 semaines »…), façon
/// GNOME qui affiche l'ancienneté plutôt qu'une date sèche. `ts` = epoch en
/// secondes ; 0 ou négatif → chaîne vide.
pub fn format_relative(ts: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_relative_from(ts, now)
}

/// Cœur testable de [`format_relative`] : « maintenant » est un paramètre.
fn format_relative_from(ts: i64, now: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    let days = (now - ts).max(0) / 86_400;
    match days {
        0 => "Aujourd'hui".to_string(),
        1 => "Hier".to_string(),
        2..=6 => format!("Il y a {days} jours"),
        7..=13 => "Il y a une semaine".to_string(),
        14..=29 => format!("Il y a {} semaines", days / 7),
        30..=59 => "Il y a un mois".to_string(),
        60..=364 => format!("Il y a {} mois", days / 30),
        365..=729 => "Il y a un an".to_string(),
        _ => format!("Il y a {} ans", days / 365),
    }
}

#[cfg(test)]
mod tests {
    use super::format_relative_from;

    /// `now` arbitraire ; on décale `ts` de N jours en arrière.
    fn ago(days: i64) -> String {
        const NOW: i64 = 1_800_000_000;
        format_relative_from(NOW - days * 86_400, NOW)
    }

    #[test]
    fn relative_dates_at_boundaries() {
        assert_eq!(ago(0), "Aujourd'hui");
        assert_eq!(ago(1), "Hier");
        assert_eq!(ago(6), "Il y a 6 jours");
        assert_eq!(ago(7), "Il y a une semaine");
        assert_eq!(ago(13), "Il y a une semaine");
        assert_eq!(ago(14), "Il y a 2 semaines");
        assert_eq!(ago(29), "Il y a 4 semaines");
        assert_eq!(ago(30), "Il y a un mois");
        assert_eq!(ago(60), "Il y a 2 mois");
        assert_eq!(ago(365), "Il y a un an");
        assert_eq!(ago(730), "Il y a 2 ans");
    }

    #[test]
    fn unknown_timestamp_is_empty() {
        assert_eq!(format_relative_from(0, 1_800_000_000), "");
        assert_eq!(format_relative_from(-5, 1_800_000_000), "");
    }

    #[test]
    fn future_timestamp_is_today() {
        // Une release datée dans le futur ne doit pas produire un nombre négatif.
        assert_eq!(format_relative_from(1_900_000_000, 1_800_000_000), "Aujourd'hui");
    }

    #[test]
    fn app_item_from_search_preserves_recognized_without_icon() {
        // Une app reconnue par AppStream mais sans icône doit rester une
        // « Application » (le split de la recherche lit `recognized`).
        let r = onyx_core::SearchResult {
            package: onyx_core::Package::from_attr("gedit"),
            icon_url: None,
            rating: None,
            recognized: true,
        };
        let item = super::AppItem::from_search(r);
        assert!(item.recognized);
        assert!(item.icon_url.is_none());
    }
}

/// Glyphe de l'icône fallback (quand AppStream/Flathub ne fournit pas d'icône).
pub const FALLBACK_GLYPH: &str = "application-x-executable-symbolic";

/// Classe CSS de couleur **déterministe** (hash du nom) pour la vignette
/// fallback : un fond coloré arrondi sur lequel on pose [`FALLBACK_GLYPH`] en
/// blanc. Couleurs définies dans `APP_CSS` (`.fb-0`..`.fb-11`).
pub fn fallback_class(name: &str) -> &'static str {
    const C: [&str; 12] = [
        "fb-0", "fb-1", "fb-2", "fb-3", "fb-4", "fb-5",
        "fb-6", "fb-7", "fb-8", "fb-9", "fb-10", "fb-11",
    ];
    let h = name.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    C[(h as usize) % C.len()]
}

/// Une app affichable : le paquet nix + une URL d'icône Flathub optionnelle
/// (renseignée pour catégorie/populaires, `None` pour la recherche texte) + une
/// note ODRS optionnelle (renseignée seulement par la recherche, comme GNOME).
/// Unité de donnée commune aux listes (`PackageRow`) et grilles (`AppTile`).
#[derive(Debug, Clone)]
pub struct AppItem {
    pub package: Package,
    pub icon_url: Option<String>,
    pub rating: Option<onyx_core::Rating>,
    /// Apparié à une entrée AppStream — critère de la section « Applications »
    /// de la recherche (une app reconnue sans icône reste une application).
    pub recognized: bool,
}

impl AppItem {
    pub fn new(package: Package, icon_url: Option<String>) -> Self {
        // Grilles accueil/catégories : tout vient de Flathub, l'icône vaut
        // reconnaissance (le split de la recherche n'y passe pas).
        let recognized = icon_url.is_some();
        Self { package, icon_url, rating: None, recognized }
    }

    pub fn from_search(r: onyx_core::SearchResult) -> Self {
        Self {
            package: r.package,
            icon_url: r.icon_url,
            rating: r.rating,
            recognized: r.recognized,
        }
    }
}

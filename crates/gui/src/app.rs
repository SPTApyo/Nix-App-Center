//! `AppModel` — la coquille : fenêtre, header (ViewSwitcher + recherche + burger),
//! pile accueil/résultats, onglets, et navigation (push catégorie/détail/essayées).
//! N'orchestre que des composants : il ne dessine aucune liste/grille lui-même.

use adw::prelude::*;
use relm4::prelude::*;
use onyx_core::Package;

use crate::components::{AppList, AppListInput};
use crate::i18n::t;
use crate::util::AppItem;
use crate::views::home::HomeOutput;
use crate::views::installed::InstalledMsg;
use crate::views::updates::UpdatesMsg;
use crate::views::{CategoryModel, DetailModel, HomeModel, InstalledModel, UpdatesModel};

/// Feuille de style de l'app : `crates/gui/data/style.css`. Les règles portées
/// de GNOME Software y sont annotées avec leur ligne source.
const APP_CSS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/style.css"));

pub struct AppModel {
    // Gardés vivants pour la durée de l'app (widgets placés dans le stack/onglets) ;
    // pilotés à l'init ou via senders clonés.
    #[allow(dead_code)]
    home: Controller<HomeModel>,
    results: Controller<AppList>,
    #[allow(dead_code)]
    installed: Controller<InstalledModel>,
    #[allow(dead_code)]
    updates: Controller<UpdatesModel>,
    #[cfg(feature = "dev-gallery")]
    #[allow(dead_code)]
    gallery: Controller<crate::views::gallery::GalleryModel>,
    stack: gtk::Stack,          // home / results (onglet Explorer)
    view_stack: adw::ViewStack, // Explorer / Installées / Mises à jour
    nav: adw::NavigationView,   // racine "main" + pages poussées
    search_entry: gtk::SearchEntry,
    search_btn: gtk::ToggleButton,
    details: Vec<Controller<DetailModel>>,
    categories: Vec<Controller<CategoryModel>>,
}

#[derive(Debug)]
pub enum AppMsg {
    Search(String),
    SearchDone(Vec<onyx_core::SearchResult>),
    LocateDone(String, Vec<Package>),
    Home(HomeOutput),
    OpenCategory(&'static [&'static str], &'static str),
    OpenDetail(Package),
    /// Une page a été dépilée (flèche retour) : libère son contrôleur.
    PagePopped,
    GoHome,
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = ();
    type Input = AppMsg;
    type Output = ();

    view! {
        adw::ApplicationWindow {
            set_title: Some("Onyx"),
            set_default_width: 960,
            set_default_height: 680,
        }
    }

    fn init(_: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        // Lance le chargement de la base AppStream locale en arrière-plan (Flatpak
        // cache). Dès que prête, toutes les recherches et pages détail sont
        // instantanées — aucune requête réseau Flathub.
        onyx_core::init_appstream_db();

        // ── CSS ───────────────────────────────────────────────────────────────
        let css = gtk::CssProvider::new();
        css.load_from_data(APP_CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        // Icônes empruntées à GNOME Software (onglets + catégories), bundlées dans
        // crates/gui/data/icons → résolues par nom via ce chemin de recherche.
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::IconTheme::for_display(&display)
                .add_search_path(concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons"));
        }
        // Icône de la fenêtre / de la barre des tâches : résolue par app-id depuis le
        // chemin ajouté ci-dessus (data/icons/hicolor/scalable/apps/dev.onyx.Onyx.svg).
        gtk::Window::set_default_icon_name("dev.onyx.Onyx");

        // ── Composants enfants ────────────────────────────────────────────────
        let home = HomeModel::builder()
            .launch(())
            .forward(sender.input_sender(), AppMsg::Home);
        let results = AppList::builder()
            .launch(())
            .forward(sender.input_sender(), AppMsg::OpenDetail);
        let installed = InstalledModel::builder()
            .launch(())
            .forward(sender.input_sender(), AppMsg::OpenDetail);
        let updates = UpdatesModel::builder()
            .launch(())
            .forward(sender.input_sender(), AppMsg::OpenDetail);

        #[cfg(feature = "dev-gallery")]
        let gallery = crate::views::gallery::GalleryModel::builder()
            .launch(())
            .forward(sender.input_sender(), |out| match out {
                crate::views::gallery::GalleryOutput::Search(q) => AppMsg::Search(q),
                crate::views::gallery::GalleryOutput::OpenDetail(p) => AppMsg::OpenDetail(p),
                crate::views::gallery::GalleryOutput::Category(c, l) => AppMsg::OpenCategory(c, l),
            });

        // ── ViewStack + Header ────────────────────────────────────────────────
        let view_stack = adw::ViewStack::new();
        let header = adw::HeaderBar::new();

        let search_btn = gtk::ToggleButton::builder()
            .icon_name("system-search-symbolic")
            .tooltip_text(t("Rechercher"))
            .build();
        header.pack_start(&search_btn);

        let switcher = adw::ViewSwitcher::new();
        switcher.set_policy(adw::ViewSwitcherPolicy::Wide);
        switcher.set_stack(Some(&view_stack));
        header.set_title_widget(Some(&switcher));

        // ── Barre de recherche ────────────────────────────────────────────────
        // Barre de recherche : mêmes dimensions que GNOME Software (gs-shell.ui) —
        // AdwClamp hexpand, maximum-size 500, tightening 500.
        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text(t("Rechercher des paquets Nix…"))
            .hexpand(true)
            .build();
        let entry_clamp = adw::Clamp::new();
        entry_clamp.set_hexpand(true);
        entry_clamp.set_maximum_size(500);
        entry_clamp.set_tightening_threshold(500);
        entry_clamp.set_child(Some(&search_entry));
        let search_bar = gtk::SearchBar::new();
        search_bar.set_child(Some(&entry_clamp));
        search_bar.connect_entry(&search_entry);
        // Type-to-search : n'importe quelle frappe dans la fenêtre ouvre la barre
        // et y route la saisie (comme GNOME : key-capture-widget = la fenêtre).
        search_bar.set_key_capture_widget(Some(&root));

        search_btn.connect_toggled({
            let sb = search_bar.clone();
            let se = search_entry.clone();
            move |btn| {
                let on = btn.is_active();
                sb.set_search_mode(on);
                if on {
                    se.grab_focus();
                }
            }
        });
        search_bar.connect_notify_local(Some("search-mode-enabled"), {
            let btn = search_btn.clone();
            let sender = sender.clone();
            move |bar, _| {
                let on = bar.is_search_mode();
                if btn.is_active() != on {
                    btn.set_active(on);
                }
                if !on {
                    sender.input(AppMsg::GoHome);
                }
            }
        });
        search_entry.connect_activate({
            let sender = sender.clone();
            move |e| {
                let q = e.text().to_string();
                if !q.trim().is_empty() {
                    sender.input(AppMsg::Search(q));
                }
            }
        });
        // NB : pas de GoHome sur search-changed — à l'ouverture (type-to-search)
        // l'entry est transitoirement vide, ce qui refermait tout. Le retour à
        // l'accueil se fait à la fermeture de la barre (search-mode-enabled=false).

        // ── Pile accueil / résultats (onglet Explorer) ────────────────────────
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(150)
            .build();
        stack.add_named(home.widget(), Some("home"));
        stack.add_named(results.widget(), Some("results"));
        stack.set_visible_child_name("home");

        // ── Onglets ───────────────────────────────────────────────────────────
        let explorer_page = view_stack.add_titled(&stack, Some("explorer"), &t("Explorer"));
        explorer_page.set_icon_name(Some("software-explore-symbolic"));

        let installed_page =
            view_stack.add_titled(installed.widget(), Some("installed"), &t("Installées"));
        installed_page.set_icon_name(Some("software-installed-symbolic"));

        let updates_page =
            view_stack.add_titled(updates.widget(), Some("updates"), &t("Mises à jour"));
        updates_page.set_icon_name(Some("software-updates-symbolic"));

        #[cfg(feature = "dev-gallery")]
        {
            // Icône dev quelconque du thème système ; si absente, l'onglet
            // s'affiche sans icône (sans conséquence, c'est du dev).
            let gallery_page =
                view_stack.add_titled(gallery.widget(), Some("gallery"), "Galerie");
            gallery_page.set_icon_name(Some("applications-engineering-symbolic"));
        }

        // Recharge les onglets Installées / Mises à jour quand on y entre.
        view_stack.connect_visible_child_notify({
            let si = installed.sender().clone();
            let su = updates.sender().clone();
            move |vs| match vs.visible_child_name().as_deref() {
                Some("installed") => {
                    let _ = si.send(InstalledMsg::Reload);
                }
                Some("updates") => {
                    let _ = su.send(UpdatesMsg::Reload);
                }
                _ => {}
            }
        });

        // ── ToolbarView + NavigationView ──────────────────────────────────────
        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.add_top_bar(&search_bar);
        toolbar_view.set_content(Some(&view_stack));

        let nav = adw::NavigationView::new();
        let root_page = adw::NavigationPage::builder()
            .title("Onyx")
            .tag("main")
            .child(&toolbar_view)
            .build();
        nav.add(&root_page);

        // Sans ça, les contrôleurs des pages dépilées (flèche retour) restaient
        // vivants jusqu'au prochain GoHome — textures de screenshots comprises.
        nav.connect_popped({
            let sender = sender.clone();
            move |_, _| sender.input(AppMsg::PagePopped)
        });

        let model = AppModel {
            home,
            results,
            installed,
            updates,
            #[cfg(feature = "dev-gallery")]
            gallery,
            stack: stack.clone(),
            view_stack: view_stack.clone(),
            nav: nav.clone(),
            search_entry: search_entry.clone(),
            search_btn: search_btn.clone(),
            details: Vec::new(),
            categories: Vec::new(),
        };

        let widgets = view_output!();
        root.set_content(Some(&nav));

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AppMsg::Search(query) => {
                if query.trim().is_empty() {
                    return;
                }
                self.nav.pop_to_tag("main");
                self.view_stack.set_visible_child_name("explorer");
                self.search_btn.set_active(true);
                self.search_entry.set_text(&query);
                self.results.emit(AppListInput::SetLoading);
                self.stack.set_visible_child_name("results");
                let s = sender.input_sender().clone();
                let s2 = sender.input_sender().clone();
                let q = query.clone();
                let q2 = query.clone();
                std::thread::spawn(move || {
                    let _ = s.send(AppMsg::SearchDone(onyx_core::search_apps(&q)));
                });
                std::thread::spawn(move || {
                    let _ = s2.send(AppMsg::LocateDone(
                        q2.clone(),
                        onyx_core::locate_binary(&q2),
                    ));
                });
            }
            AppMsg::SearchDone(items) => {
                let items = items.into_iter().map(AppItem::from_search).collect();
                self.results.emit(AppListInput::Set(items));
            }
            AppMsg::LocateDone(query, packages) => {
                self.results.emit(AppListInput::SetLocate(query, packages));
            }
            AppMsg::Home(HomeOutput::Category(cats, label)) => {
                sender.input(AppMsg::OpenCategory(cats, label));
            }
            AppMsg::Home(HomeOutput::Open(pkg)) => {
                sender.input(AppMsg::OpenDetail(pkg));
            }
            AppMsg::OpenCategory(cats, label) => {
                let controller = CategoryModel::builder()
                    .launch((cats, label.to_string()))
                    .forward(sender.input_sender(), AppMsg::OpenDetail);
                self.nav.push(controller.widget());
                self.categories.push(controller);
            }
            AppMsg::OpenDetail(pkg) => {
                // forward (≠ detach) : la page détail peut demander l'ouverture
                // d'une autre app (tuiles « Autres applications par X »).
                let controller = DetailModel::builder()
                    .launch(pkg)
                    .forward(sender.input_sender(), AppMsg::OpenDetail);
                self.nav.push(controller.widget());
                self.details.push(controller);
            }
            AppMsg::PagePopped => {
                // Une page dépilée est retirée de la NavigationView (plus de
                // parent) : c'est le critère pour libérer son contrôleur.
                self.details.retain(|c| c.widget().parent().is_some());
                self.categories.retain(|c| c.widget().parent().is_some());
            }
            AppMsg::GoHome => {
                self.nav.pop_to_tag("main");
                self.details.clear();
                self.categories.clear();
                self.stack.set_visible_child_name("home");
                if self.search_btn.is_active() {
                    self.search_btn.set_active(false);
                }
            }
        }
    }
}

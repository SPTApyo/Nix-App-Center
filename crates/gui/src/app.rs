//! `AppModel`: the shell. Window, header (view switcher + search), home/results
//! stack, tabs, and navigation (pushing category and details pages).
//! It only orchestrates components and never draws a list or grid itself.

use adw::prelude::*;
use onyx_core::Package;
use relm4::prelude::*;

use crate::components::{AppList, AppListInput};
use crate::i18n::t;
use crate::util::AppItem;
use crate::views::home::HomeOutput;
use crate::views::installed::InstalledMsg;
use crate::views::updates::UpdatesMsg;
use crate::views::{CategoryModel, DetailModel, HomeModel, InstalledModel, UpdatesModel};

/// App stylesheet: `crates/gui/data/style.css`. Rules ported from GNOME
/// Software are annotated with their source line.
const APP_CSS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/style.css"));

pub struct AppModel {
    // Kept alive for the app lifetime (their widgets live in the stack/tabs),
    // driven at init or through cloned senders.
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
    stack: gtk::Stack,          // home / results (Explore tab)
    view_stack: adw::ViewStack, // Explore / Installed / Updates
    nav: adw::NavigationView,   // "main" root + pushed pages
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
    /// A page was popped (back arrow): release its controller.
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

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Load the AppStream database in the background. Once ready, searches
        // and details pages need no Flathub request.
        onyx_core::init_appstream_db();

        // ── CSS ───────────────────────────────────────────────────────────────
        let css = gtk::CssProvider::new();
        css.load_from_data(APP_CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        // Bundled icons (tabs, categories, tiles). Installed builds ship them in
        // share/icons; `cargo run` reads them from the source tree.
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::IconTheme::for_display(&display)
                .add_search_path(concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons"));
        }
        // Window / taskbar icon, resolved by app id from the path added above
        // (data/icons/hicolor/scalable/apps/dev.onyx.Onyx.svg).
        gtk::Window::set_default_icon_name("dev.onyx.Onyx");

        // ── Child components ──────────────────────────────────────────────────
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

        // ── View stack + header ───────────────────────────────────────────────
        let view_stack = adw::ViewStack::new();
        let header = adw::HeaderBar::new();

        let search_btn = gtk::ToggleButton::builder()
            .icon_name("system-search-symbolic")
            .tooltip_text(t("Search"))
            .build();
        header.pack_start(&search_btn);

        let switcher = adw::ViewSwitcher::new();
        switcher.set_policy(adw::ViewSwitcherPolicy::Wide);
        switcher.set_stack(Some(&view_stack));
        header.set_title_widget(Some(&switcher));

        // ── Search bar ────────────────────────────────────────────────────────
        // Same dimensions as GNOME Software (gs-shell.ui): AdwClamp hexpand,
        // maximum-size 500, tightening 500.
        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text(t("Search Nix packages…"))
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
        // Type-to-search: any key press in the window opens the bar and routes
        // the input there (GNOME uses the window as key-capture-widget).
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
        // No GoHome on search-changed: when type-to-search opens the bar the
        // entry is briefly empty, which used to close everything. Going home
        // happens when the bar closes (search-mode-enabled=false).

        // ── Home / results stack (Explore tab) ────────────────────────────────
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(150)
            .build();
        stack.add_named(home.widget(), Some("home"));
        stack.add_named(results.widget(), Some("results"));
        stack.set_visible_child_name("home");

        // ── Tabs ──────────────────────────────────────────────────────────────
        let explorer_page = view_stack.add_titled(&stack, Some("explorer"), &t("Explore"));
        explorer_page.set_icon_name(Some("software-explore-symbolic"));

        let installed_page =
            view_stack.add_titled(installed.widget(), Some("installed"), &t("Installed"));
        installed_page.set_icon_name(Some("software-installed-symbolic"));

        let updates_page = view_stack.add_titled(updates.widget(), Some("updates"), &t("Updates"));
        updates_page.set_icon_name(Some("software-updates-symbolic"));

        #[cfg(feature = "dev-gallery")]
        {
            // Any developer icon from the system theme; when missing the tab
            // simply has no icon (dev only).
            let gallery_page = view_stack.add_titled(gallery.widget(), Some("gallery"), "Gallery");
            gallery_page.set_icon_name(Some("applications-engineering-symbolic"));
        }

        // Reload the Installed / Updates tabs when entering them.
        view_stack.connect_visible_child_notify({
            let si = installed.sender().clone();
            let su = updates.sender().clone();
            let search_btn = search_btn.clone();
            move |vs| {
                let tab = vs.visible_child_name();
                // Search belongs to Explore: do not leave stale results open.
                if tab.as_deref() != Some("explorer") {
                    search_btn.set_active(false);
                }
                match tab.as_deref() {
                    Some("installed") => {
                        let _ = si.send(InstalledMsg::Reload);
                    }
                    Some("updates") => {
                        let _ = su.send(UpdatesMsg::Reload);
                    }
                    _ => {}
                }
            }
        });

        // ── Toolbar view + navigation view ────────────────────────────────────
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

        // Without this, controllers of popped pages (back arrow) stayed alive
        // until the next GoHome, screenshot textures included.
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
                // forward (not detach): a details page may open another app
                // ("Other apps by X" tiles).
                let controller = DetailModel::builder()
                    .launch(pkg)
                    .forward(sender.input_sender(), AppMsg::OpenDetail);
                self.nav.push(controller.widget());
                self.details.push(controller);
            }
            AppMsg::PagePopped => {
                // A popped page is removed from the NavigationView (no parent
                // anymore): that is the signal to release its controller.
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

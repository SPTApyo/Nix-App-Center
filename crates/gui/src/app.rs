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
use crate::views::storage::StorageMsg;
use crate::views::updates::{UpdatesMsg, UpdatesOutput};
use crate::views::{
    CategoryModel, DetailModel, HomeModel, InstalledModel, StorageModel, UpdatesModel,
};

/// App stylesheet: `crates/gui/data/style.css`. Rules ported from GNOME
/// Software are annotated with their source line.
const APP_CSS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/style.css"));

pub struct AppModel {
    // Kept alive for the app lifetime (their widgets live in the stack/tabs),
    // driven at init or through cloned senders.
    _home: Controller<HomeModel>,
    results: Controller<AppList>,
    _installed: Controller<InstalledModel>,
    _updates: Controller<UpdatesModel>,
    _storage: Controller<StorageModel>,
    #[cfg(feature = "dev-gallery")]
    _gallery: Controller<crate::views::gallery::GalleryModel>,
    stack: gtk::Stack,          // home / results (Explore tab)
    view_stack: adw::ViewStack, // Explore / Installed / Updates
    updates_page: adw::ViewStackPage,
    nav: adw::NavigationView, // "main" root + pushed pages
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
    OptionsDone(Vec<onyx_core::NixOption>),
    FileTypesDone(String, Vec<Package>),
    Home(HomeOutput),
    OpenCategory(&'static [&'static str], &'static str),
    OpenDetail(Package),
    /// A page was popped (back arrow): release its controller.
    PagePopped,
    GoHome,
    Updates(UpdatesOutput),
    ShowUpdates,
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = ();
    type Input = AppMsg;
    type Output = ();

    view! {
        adw::ApplicationWindow {
            set_title: Some("Nix Software"),
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
            .forward(sender.input_sender(), AppMsg::Updates);
        let storage = StorageModel::builder().launch(()).detach();

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

        // Main menu, like every GNOME app.
        let menu = gtk::gio::Menu::new();
        menu.append(Some(&t("_Preferences")), Some("win.preferences"));
        menu.append(Some(&t("_About Nix Software")), Some("win.about"));
        let menu_btn = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .primary(true)
            .tooltip_text(t("Main Menu"))
            .build();
        header.pack_end(&menu_btn);
        let prefs = gtk::gio::SimpleAction::new("preferences", None);
        prefs.connect_activate({
            let win = root.clone();
            move |_, _| crate::views::preferences::present(&win)
        });
        root.add_action(&prefs);
        let about = gtk::gio::SimpleAction::new("about", None);
        about.connect_activate({
            let win = root.clone();
            move |_, _| about_dialog().present(Some(&win))
        });
        root.add_action(&about);
        let app = relm4::main_application();
        app.set_accels_for_action("win.preferences", &["<Control>comma"]);
        let show_updates = gtk::gio::SimpleAction::new("show-updates", None);
        show_updates.connect_activate({
            let sender = sender.clone();
            move |_, _| sender.input(AppMsg::ShowUpdates)
        });
        app.add_action(&show_updates);

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

        let storage_page = view_stack.add_titled(storage.widget(), Some("storage"), &t("Storage"));
        storage_page.set_icon_name(Some("drive-harddisk-symbolic"));

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
            let ss = storage.sender().clone();
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
                    Some("storage") => {
                        let _ = ss.send(StorageMsg::Reload);
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
            .title("Nix Software")
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

        // Daily background check, a little after start.
        let settings = onyx_core::load_settings();
        let now = onyx_core::now_unix();
        if settings.auto_check_updates && now.saturating_sub(settings.last_update_check) > 86_400 {
            let su = updates.sender().clone();
            gtk::glib::timeout_add_seconds_local_once(30, move || {
                let _ = su.send(UpdatesMsg::Background);
            });
        }

        let model = AppModel {
            _home: home,
            results,
            _installed: installed,
            _updates: updates,
            _storage: storage,
            #[cfg(feature = "dev-gallery")]
            _gallery: gallery,
            stack: stack.clone(),
            view_stack: view_stack.clone(),
            updates_page,
            nav: nav.clone(),
            search_entry: search_entry.clone(),
            search_btn: search_btn.clone(),
            details: Vec::new(),
            categories: Vec::new(),
        };

        let widgets = view_output!();
        root.set_content(Some(&nav));
        let _ = crate::search_provider::APP_SENDER.set(sender.input_sender().clone());

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
                let s4 = sender.input_sender().clone();
                let q4 = query.clone();
                std::thread::spawn(move || {
                    let apps = onyx_core::apps_for_file_type(&q4, 6)
                        .into_iter()
                        .map(|(p, _)| p)
                        .collect();
                    let _ = s4.send(AppMsg::FileTypesDone(q4, apps));
                });
                // NixOS options only make sense on NixOS.
                if onyx_core::detect_layout().system.is_some() {
                    let s3 = sender.input_sender().clone();
                    std::thread::spawn(move || {
                        let _ = s3.send(AppMsg::OptionsDone(onyx_core::search_options(&query, 6)));
                    });
                }
            }
            AppMsg::SearchDone(items) => {
                let items = items.into_iter().map(AppItem::from_search).collect();
                self.results.emit(AppListInput::Set(items));
            }
            AppMsg::LocateDone(query, packages) => {
                self.results.emit(AppListInput::SetLocate(query, packages));
            }
            AppMsg::FileTypesDone(query, packages) => {
                self.results
                    .emit(AppListInput::SetFileTypes(query, packages));
            }
            AppMsg::OptionsDone(options) => {
                self.results.emit(AppListInput::SetOptions(options));
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
            AppMsg::Updates(UpdatesOutput::Pending { count, notify }) => {
                self.updates_page.set_badge_number(count as u32);
                self.updates_page.set_needs_attention(count > 0);
                if notify {
                    let mut s = onyx_core::load_settings();
                    s.last_update_check = onyx_core::now_unix();
                    let _ = onyx_core::save_settings(&s);
                }
                if notify && count > 0 {
                    let n = gtk::gio::Notification::new(&t("Updates Available"));
                    n.set_body(Some(&crate::util::plural(
                        "{} app can be updated.",
                        "{} apps can be updated.",
                        count as i64,
                    )));
                    n.set_default_action("app.show-updates");
                    relm4::main_application().send_notification(Some("updates"), &n);
                }
            }
            AppMsg::ShowUpdates => {
                self.nav.pop_to_tag("main");
                self.view_stack.set_visible_child_name("updates");
                relm4::main_application().activate();
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

fn about_dialog() -> adw::AboutDialog {
    let d = adw::AboutDialog::builder()
        .application_name("Nix Software")
        .application_icon("dev.onyx.Onyx")
        .developer_name("SPTApyo")
        .version(env!("CARGO_PKG_VERSION"))
        .website("https://github.com/SPTApyo/Projet-Onyx")
        .issue_url("https://github.com/SPTApyo/Projet-Onyx/issues")
        .license_type(gtk::License::Gpl30)
        .copyright("© 2026 SPTApyo")
        .build();
    // The icon reuses two artworks: credit both, as their licenses ask.
    d.add_legal_section(
        &t("Icon"),
        Some(
            "Bag: GNOME Software authors (GPL-2.0-or-later). Snowflake: NixOS artwork (CC-BY-4.0).",
        ),
        gtk::License::Custom,
        None,
    );
    let credits = t("translator-credits");
    if credits != "translator-credits" {
        d.set_translator_credits(&credits);
    }
    d
}

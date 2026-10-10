//! The window: header, tabs, search, and navigation to category and
//! details pages.

use adw::prelude::*;
use nix_software_core::Package;
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

/// App stylesheet.
const APP_CSS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/style.css"));

pub struct AppModel {
    // Controllers kept alive with their widgets.
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
    SearchDone(Vec<nix_software_core::SearchResult>),
    LocateDone(String, Vec<Package>),
    OptionsDone(Vec<nix_software_core::NixOption>),
    FileTypesDone(String, Vec<Package>),
    Home(HomeOutput),
    OpenCategory(&'static [&'static str], &'static str),
    OpenDetail(Package),
    /// A page was popped: release its controller.
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
        nix_software_core::init_appstream_db();

        let css = gtk::CssProvider::new();
        css.load_from_data(APP_CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        // Bundled icons: installed next to bin, or the source tree.
        if let Some(display) = gtk::gdk::Display::default() {
            let theme = gtk::IconTheme::for_display(&display);
            if let Some(out) = std::env::current_exe()
                .ok()
                .and_then(|exe| Some(exe.parent()?.parent()?.to_path_buf()))
            {
                theme.add_search_path(out.join("share/nix-software/icons"));
            }
            theme.add_search_path(concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons"));
        }
        gtk::Window::set_default_icon_name("io.github.sptapyo.NixSoftware");

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

        let view_stack = adw::ViewStack::new();
        let header = adw::HeaderBar::new();

        let search_btn = gtk::ToggleButton::builder()
            .icon_name("system-search-symbolic")
            .tooltip_text(t("Search"))
            .build();
        header.pack_start(&search_btn);

        let menu = gtk::gio::Menu::new();
        menu.append(Some(&t("_Welcome Tour")), Some("win.welcome"));
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
        let welcome = gtk::gio::SimpleAction::new("welcome", None);
        welcome.connect_activate({
            let win = root.clone();
            move |_, _| crate::components::welcome::present(&win)
        });
        root.add_action(&welcome);
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

        // Sized like GNOME Software's search bar.
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
        // Type-to-search from anywhere in the window.
        search_bar.set_key_capture_widget(Some(&root));

        let search = gtk::gio::SimpleAction::new("search", None);
        search.connect_activate({
            let btn = search_btn.clone();
            move |_, _| btn.set_active(!btn.is_active())
        });
        root.add_action(&search);
        app.set_accels_for_action("win.search", &["<Control>f"]);
        let quit = gtk::gio::SimpleAction::new("quit", None);
        quit.connect_activate({
            let win = root.clone();
            move |_, _| win.close()
        });
        root.add_action(&quit);
        app.set_accels_for_action("win.quit", &["<Control>q"]);

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
        // Going home happens when the bar closes.

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(150)
            .build();
        stack.add_named(home.widget(), Some("home"));
        stack.add_named(results.widget(), Some("results"));
        stack.set_visible_child_name("home");

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
            let gallery_page = view_stack.add_titled(gallery.widget(), Some("gallery"), "Gallery");
            gallery_page.set_icon_name(Some("applications-engineering-symbolic"));
        }

        view_stack.connect_visible_child_notify({
            let si = installed.sender().clone();
            let su = updates.sender().clone();
            let ss = storage.sender().clone();
            let search_btn = search_btn.clone();
            move |vs| {
                let tab = vs.visible_child_name();
                // Search results belong to Explore.
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

        // Release controllers of popped pages.
        nav.connect_popped({
            let sender = sender.clone();
            move |_, _| sender.input(AppMsg::PagePopped)
        });

        let settings = nix_software_core::load_settings();
        if !settings.onboarded {
            let win = root.clone();
            gtk::glib::idle_add_local_once(move || crate::components::welcome::present(&win));
        }
        let now = nix_software_core::now_unix();
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
                    let _ = s.send(AppMsg::SearchDone(nix_software_core::search_apps(&q)));
                });
                std::thread::spawn(move || {
                    let _ = s2.send(AppMsg::LocateDone(
                        q2.clone(),
                        nix_software_core::locate_binary(&q2),
                    ));
                });
                let s4 = sender.input_sender().clone();
                let q4 = query.clone();
                std::thread::spawn(move || {
                    let apps = nix_software_core::apps_for_file_type(&q4, 6)
                        .into_iter()
                        .map(|(p, _)| p)
                        .collect();
                    let _ = s4.send(AppMsg::FileTypesDone(q4, apps));
                });
                if nix_software_core::detect_layout().system.is_some() {
                    let s3 = sender.input_sender().clone();
                    std::thread::spawn(move || {
                        let _ = s3.send(AppMsg::OptionsDone(nix_software_core::search_options(
                            &query, 6,
                        )));
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
                // Forwarded: a details page may open another app.
                let controller = DetailModel::builder()
                    .launch(pkg)
                    .forward(sender.input_sender(), AppMsg::OpenDetail);
                self.nav.push(controller.widget());
                self.details.push(controller);
            }
            AppMsg::PagePopped => {
                // Popped pages have no parent.
                self.details.retain(|c| c.widget().parent().is_some());
                self.categories.retain(|c| c.widget().parent().is_some());
            }
            AppMsg::Updates(UpdatesOutput::Pending { count, notify }) => {
                self.updates_page.set_badge_number(count as u32);
                self.updates_page.set_needs_attention(count > 0);
                if notify {
                    let mut s = nix_software_core::load_settings();
                    s.last_update_check = nix_software_core::now_unix();
                    let _ = nix_software_core::save_settings(&s);
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
        .application_icon("io.github.sptapyo.NixSoftware")
        .developer_name("SPTApyo")
        .version(env!("CARGO_PKG_VERSION"))
        .website("https://github.com/SPTApyo/nix-software")
        .issue_url("https://github.com/SPTApyo/nix-software/issues")
        .license_type(gtk::License::Gpl30)
        .copyright("© 2026 SPTApyo")
        .build();
    // Credit both artworks of the icon.
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

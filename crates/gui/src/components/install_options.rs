//! Installation options of a kept app: where it goes (home or system) and
//! how (plain package or module).

use adw::prelude::*;
use nix_software_core::InstallMode;

use crate::i18n::t;
use crate::util::esc;

/// What the app can do on this machine; `None` hides a choice.
pub struct Offer {
    pub home: bool,
    pub system: bool,
    /// The NixOS module's `enable` option.
    pub module: Option<String>,
    /// The Home Manager module's `enable` option.
    pub home_module: Option<String>,
}

impl Offer {
    /// True when the app has a module: the user should pick.
    pub fn has_choice(&self) -> bool {
        self.module.is_some() || self.home_module.is_some()
    }
}

/// The mode a choice of place and kind maps to.
fn mode_for(offer: &Offer, everyone: bool, module: bool) -> Option<InstallMode> {
    match (everyone, module) {
        (false, false) => offer.home.then_some(InstallMode::Home),
        (true, false) => offer.system.then_some(InstallMode::System),
        (false, true) => offer.home_module.clone().map(InstallMode::HomeModule),
        (true, true) => offer.module.clone().map(InstallMode::Module),
    }
}

/// Asks how to install an app being added, `default` selected; `on_add`
/// gets the choice.
pub fn present_new(
    parent: &impl IsA<gtk::Widget>,
    offer: Offer,
    default: InstallMode,
    on_add: impl Fn(InstallMode) + 'static,
) {
    show(parent, offer, Some(default), true, on_add);
}

/// Shows the options with `current` selected; `on_save` gets the new mode.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    offer: Offer,
    current: Option<InstallMode>,
    on_save: impl Fn(InstallMode) + 'static,
) {
    show(parent, offer, current, false, on_save);
}

fn show(
    parent: &impl IsA<gtk::Widget>,
    offer: Offer,
    current: Option<InstallMode>,
    adding: bool,
    on_save: impl Fn(InstallMode) + 'static,
) {
    let everyone_now = matches!(current, Some(InstallMode::System | InstallMode::Module(_)));
    let module_now = matches!(
        current,
        Some(InstallMode::Module(_) | InstallMode::HomeModule(_))
    );

    let radio = |title: String, subtitle: String, group: Option<&gtk::CheckButton>| {
        let check = gtk::CheckButton::new();
        check.set_group(group);
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(esc(&subtitle))
            .activatable_widget(&check)
            .build();
        row.add_prefix(&check);
        (row, check)
    };

    let place = adw::PreferencesGroup::builder().title(t("Where")).build();
    let (home_row, home) = radio(t("In Your Home"), t("Only you get the app."), None);
    let (system_row, system) = radio(
        t("For Everyone"),
        t("The app goes to the system packages, for every user of this computer."),
        Some(&home),
    );
    home_row.set_sensitive(offer.home || offer.home_module.is_some());
    system_row.set_sensitive(offer.system || offer.module.is_some());
    place.add(&home_row);
    place.add(&system_row);

    let kind = adw::PreferencesGroup::builder().title(t("How")).build();
    let (package_row, package) = radio(t("As a Package"), t("Just the app."), None);
    let (module_row, module) = radio(t("Through Its Module"), String::new(), Some(&package));
    kind.add(&package_row);
    kind.add(&module_row);

    let save = gtk::Button::with_mnemonic(&if adding {
        t("_Add to Configuration")
    } else {
        t("_Save")
    });
    save.add_css_class("suggested-action");
    save.add_css_class("pill");
    save.set_halign(gtk::Align::Center);

    let offer = std::rc::Rc::new(offer);
    // Module text and availability follow the place.
    let refresh = {
        let (offer, system, module, module_row, package, save) = (
            offer.clone(),
            system.clone(),
            module.clone(),
            module_row.clone(),
            package.clone(),
            save.clone(),
        );
        let current = current.clone();
        move || {
            let everyone = system.is_active();
            let option = if everyone {
                offer.module.clone()
            } else {
                offer.home_module.clone()
            };
            module_row.set_subtitle(&esc(&match &option {
                Some(o) => {
                    t("{} also sets up what the app needs around it, such as services or groups.")
                        .replace("{}", o)
                }
                None => t("No module for this app here."),
            }));
            module_row.set_sensitive(option.is_some());
            if option.is_none() && module.is_active() {
                package.set_active(true);
            }
            let chosen = mode_for(&offer, everyone, module.is_active());
            save.set_sensitive(chosen.is_some() && (adding || chosen != current));
        }
    };
    if everyone_now {
        system.set_active(true)
    } else {
        home.set_active(true)
    }
    if module_now {
        module.set_active(true)
    } else {
        package.set_active(true)
    }
    for check in [&home, &system, &package, &module] {
        let refresh = refresh.clone();
        check.connect_toggled(move |_| refresh());
    }
    refresh();

    let body = gtk::Box::new(gtk::Orientation::Vertical, 18);
    body.set_margin_top(12);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);
    body.append(&place);
    body.append(&kind);
    body.append(&save);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&body));
    let dialog = adw::Dialog::builder()
        .title(t("Installation Options"))
        .content_width(480)
        .child(&toolbar)
        .build();
    save.connect_clicked({
        let dialog = dialog.clone();
        move |_| {
            if let Some(mode) = mode_for(&offer, system.is_active(), module.is_active()) {
                on_save(mode);
            }
            dialog.close();
        }
    });
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_and_kind_give_the_mode() {
        let offer = Offer {
            home: true,
            system: true,
            module: Some("programs.steam.enable".into()),
            home_module: None,
        };
        assert_eq!(mode_for(&offer, false, false), Some(InstallMode::Home));
        assert_eq!(
            mode_for(&offer, true, true),
            Some(InstallMode::Module("programs.steam.enable".into()))
        );
        assert_eq!(mode_for(&offer, false, true), None);
    }
}

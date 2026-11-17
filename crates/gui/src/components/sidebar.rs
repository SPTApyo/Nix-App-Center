//! A dialog body with a sidebar of pages, shared by Preferences and
//! System Settings.

use adw::prelude::*;

/// One sidebar entry and the page it shows.
pub struct Section {
    pub key: String,
    pub title: String,
    pub icon: &'static str,
    pub page: gtk::Widget,
    /// Shown at the end of the entry, such as a changed-dot.
    pub suffix: Option<gtk::Widget>,
}

/// The sidebar, a separator and the pages; the first entry is selected.
pub fn body(sections: Vec<Section>, footer: Option<gtk::Widget>) -> gtk::Box {
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_hexpand(true);
    let nav = gtk::ListBox::new();
    nav.add_css_class("navigation-sidebar");
    for section in sections {
        stack.add_named(&section.page, Some(&section.key));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.set_margin_top(4);
        row.set_margin_bottom(4);
        row.append(&gtk::Image::from_icon_name(section.icon));
        let label = gtk::Label::new(Some(&section.title));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row.append(&label);
        if let Some(suffix) = &section.suffix {
            row.append(suffix);
        }
        let item = gtk::ListBoxRow::new();
        item.set_child(Some(&row));
        item.set_widget_name(&section.key);
        nav.append(&item);
    }
    nav.connect_row_selected({
        let stack = stack.clone();
        move |_, row| {
            if let Some(row) = row {
                stack.set_visible_child_name(&row.widget_name());
            }
        }
    });
    nav.select_row(nav.row_at_index(0).as_ref());

    let side = gtk::Box::new(gtk::Orientation::Vertical, 0);
    side.set_size_request(240, -1);
    // Rows expand inside; the sidebar itself must not.
    side.set_hexpand(false);
    side.append(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&nav)
            .build(),
    );
    if let Some(footer) = footer {
        footer.set_margin_top(12);
        footer.set_margin_bottom(12);
        footer.set_margin_start(18);
        footer.set_margin_end(18);
        side.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        side.append(&footer);
    }
    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.append(&side);
    body.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    body.append(&stack);
    body
}

/// The large title and one-line summary opening a page.
pub fn header(title: &str, summary: &str) -> adw::PreferencesGroup {
    let label = gtk::Label::new(Some(title));
    label.add_css_class("title-1");
    label.set_xalign(0.0);
    let head = gtk::Box::new(gtk::Orientation::Vertical, 4);
    head.append(&label);
    if !summary.is_empty() {
        head.append(&crate::util::dim_label(summary));
    }
    let group = adw::PreferencesGroup::new();
    group.add(&head);
    group
}

/// A caption for the sidebar's footer.
pub fn footer(text: &str) -> gtk::Widget {
    let label = crate::util::dim_label(text);
    label.add_css_class("caption");
    label.set_max_width_chars(28);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.upcast()
}

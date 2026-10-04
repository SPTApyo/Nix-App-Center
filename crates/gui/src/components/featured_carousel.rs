//! `FeaturedCarousel`: the rotating featured banner of the home page (copy of
//! `GsFeaturedCarousel`: AdwCarousel `.card`, height 318, round prev/next
//! buttons, indicator dots). Slides are Flathub apps on a gradient background.

use adw::prelude::*;
use relm4::prelude::*;
use onyx_core::Package;

use crate::util::{texture_from_bytes, AppItem};

pub struct FeaturedCarousel {
    carousel: adw::Carousel,
    slides: Vec<gtk::Widget>,
    images: Vec<gtk::Image>,
}

#[derive(Debug)]
pub enum FeaturedInput {
    Set(Vec<AppItem>),
    Open(Package),
    Prev,
    Next,
    IconLoaded(usize, Vec<u8>),
}

#[relm4::component(pub)]
impl SimpleComponent for FeaturedCarousel {
    type Init = ();
    type Input = FeaturedInput;
    type Output = Package;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 12,
            set_height_request: 318,

            gtk::Overlay {
                set_vexpand: true,

                #[local_ref]
                carousel -> adw::Carousel {
                    set_vexpand: true,
                    add_css_class: "card",
                },

                add_overlay = &gtk::Button {
                    set_icon_name: "go-previous-symbolic",
                    set_halign: gtk::Align::Start,
                    set_valign: gtk::Align::Center,
                    set_margin_start: 9,
                    add_css_class: "circular",
                    add_css_class: "flat",
                    connect_clicked => FeaturedInput::Prev,
                },
                add_overlay = &gtk::Button {
                    set_icon_name: "go-next-symbolic",
                    set_halign: gtk::Align::End,
                    set_valign: gtk::Align::Center,
                    set_margin_end: 9,
                    add_css_class: "circular",
                    add_css_class: "flat",
                    connect_clicked => FeaturedInput::Next,
                },
            },

            #[local_ref]
            dots -> adw::CarouselIndicatorDots {
                set_carousel: Some(&carousel),
            },
        }
    }

    fn init(_: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let carousel = adw::Carousel::new();
        let dots = adw::CarouselIndicatorDots::new();

        let model = FeaturedCarousel {
            carousel: carousel.clone(),
            slides: Vec::new(),
            images: Vec::new(),
        };
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            FeaturedInput::Set(items) => {
                for w in self.slides.drain(..) {
                    self.carousel.remove(&w);
                }
                self.images.clear();

                for (idx, item) in items.into_iter().take(6).enumerate() {
                    let (slide, img) = build_slide(&item, idx, &sender);
                    self.carousel.append(&slide);
                    self.slides.push(slide.upcast());
                    self.images.push(img);

                    if let Some(url) = item.icon_url {
                        let s = sender.input_sender().clone();
                        std::thread::spawn(move || {
                            if let Some(bytes) = onyx_core::cached_fetch_bytes(&url) {
                                let _ = s.send(FeaturedInput::IconLoaded(idx, bytes));
                            }
                        });
                    }
                }
            }
            FeaturedInput::Open(pkg) => {
                let _ = sender.output(pkg);
            }
            FeaturedInput::Prev => self.scroll(-1),
            FeaturedInput::Next => self.scroll(1),
            FeaturedInput::IconLoaded(idx, bytes) => {
                if let (Some(img), Some(tex)) =
                    (self.images.get(idx), texture_from_bytes(&bytes))
                {
                    img.set_paintable(Some(&tex));
                }
            }
        }
    }
}

impl FeaturedCarousel {
    fn scroll(&self, dir: i32) {
        let n = self.carousel.n_pages() as i32;
        if n == 0 {
            return;
        }
        let pos = self.carousel.position().round() as i32;
        let target = (pos + dir).rem_euclid(n) as usize;
        if let Some(w) = self.slides.get(target) {
            self.carousel.scroll_to(w, true);
        }
    }
}

fn build_slide(
    item: &AppItem,
    idx: usize,
    sender: &ComponentSender<FeaturedCarousel>,
) -> (gtk::Button, gtk::Image) {
    let img = gtk::Image::builder().pixel_size(96).build();
    img.set_icon_name(Some("application-x-executable-symbolic"));

    let name = gtk::Label::builder()
        .label(&item.package.name)
        .xalign(0.0)
        .wrap(true)
        .build();
    name.add_css_class("title-1");

    let summary = gtk::Label::builder()
        .label(&item.package.description)
        .xalign(0.0)
        .wrap(true)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .spacing(6)
        .build();
    text.append(&name);
    text.append(&summary);

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(28)
        .build();
    inner.append(&img);
    inner.append(&text);

    let btn = gtk::Button::builder()
        .child(&inner)
        .hexpand(true)
        .vexpand(true)
        .build();
    btn.add_css_class("carousel-slide");
    btn.add_css_class(&format!("carousel-slide-{}", idx % 6));

    let pkg = item.package.clone();
    let s = sender.clone();
    btn.connect_clicked(move |_| {
        s.input(FeaturedInput::Open(pkg.clone()));
    });

    (btn, img)
}

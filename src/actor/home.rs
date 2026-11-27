//! The home actor, on the main thread: opens and closes home's window, gives
//! it the apps and the settings (docs/home.md).
//!
//! Its sender is kept in one place, so the reactor, the wm controller and the
//! Overview reach it without each being handed it.

use std::sync::OnceLock;

use objc2_foundation::MainThreadMarker;

use crate::actor::{self, config};
use crate::common::config::{ConfigSource, HomeSettings};
use crate::ui::home::apps::{self, Catalog};
use crate::ui::home::panel::{Closed, Home};

#[derive(Debug)]
pub enum Event {
    /// Close home if it is open; otherwise open it on this display (a
    /// `CGDirectDisplayID`). `None`: the focused display shows a fullscreen
    /// app, and home does not open there.
    Toggle(Option<u32>),
    /// Something else moved the view or took over the screen.
    Close,
    Configure(HomeSettings),
    Catalog(Catalog),
}

pub type Sender = actor::Sender<Event>;

static SENDER: OnceLock<Sender> = OnceLock::new();

/// Tell home `event`; nothing before home is started.
pub fn send(event: Event) {
    if let Some(sender) = SENDER.get() {
        sender.send(event);
    }
}

pub struct HomeActor {
    rx: actor::Receiver<Event>,
    home: Home,
}

impl HomeActor {
    pub fn new(settings: HomeSettings, config_tx: config::Sender, mtm: MainThreadMarker) -> Self {
        let (tx, rx) = actor::channel();
        let _ = SENDER.set(tx);
        let home = Home::new(
            mtm,
            settings,
            move |bundle_id| hide(&config_tx, bundle_id),
            read_apps,
        );
        read_apps();
        Self { rx, home }
    }

    pub async fn run(mut self) {
        while let Some((span, event)) = self.rx.recv().await {
            let _guard = span.enter();
            match event {
                Event::Toggle(display) => {
                    if self.home.is_open() {
                        self.home.close(Closed::Itself);
                    } else if let Some(display) = display {
                        self.home.open(display);
                    }
                }
                Event::Close => self.home.close(Closed::Elsewhere),
                Event::Configure(settings) => self.home.configure(settings),
                Event::Catalog(catalog) => self.home.set_catalog(catalog),
            }
        }
    }
}

/// Read the apps on a thread of their own; they come back as `Catalog`.
fn read_apps() {
    std::thread::Builder::new()
        .name("home-apps".into())
        .spawn(|| send(Event::Catalog(apps::read())))
        .map(drop)
        .unwrap_or_else(|error| tracing::warn!(%error, "home: could not read the apps"));
}

/// Put `bundle_id` in `[settings.grid.home] hidden`, without waiting.
fn hide(config_tx: &config::Sender, bundle_id: String) {
    config_tx.send(config::Event::Edit(Box::new(
        move |source: &mut ConfigSource| {
            let home = &mut source.settings.grid.home;
            if !home.hides(&bundle_id) {
                home.hidden.push(bundle_id);
            }
        },
    )));
}

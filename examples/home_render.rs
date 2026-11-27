//! Draw home to PNGs without showing a window: `cargo run --example
//! home_render -- <dir>` writes `full.png`, `window.png` and `search.png`.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSScreen};
use rift_wm::common::config::{HomeSettings, HomeStyle};
use rift_wm::ui::home::apps;
use rift_wm::ui::home::panel::Home;

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let screen = NSScreen::mainScreen(mtm).unwrap();
    let (frame, usable) = (screen.frame(), screen.visibleFrame());
    let catalog = apps::read();
    let home = Home::new(mtm, HomeSettings::default(), |_| {}, || {});
    home.set_catalog(catalog);
    for (name, style, query) in [
        ("full", HomeStyle::Full, ""),
        ("window", HomeStyle::Window, ""),
        ("search", HomeStyle::Window, "mo"),
    ] {
        home.configure(HomeSettings {
            style,
            ..HomeSettings::default()
        });
        let path = format!("{dir}/{name}.png");
        let started = std::time::Instant::now();
        let written = home.render(frame, usable, query, &path);
        println!("{path}: {written} in {:?}", started.elapsed());
    }
}

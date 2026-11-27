//! Which kind of app each app is, for home's boxes (docs/home.md, "The grid"):
//! a category given by hand, else a browser or a browser's web app is
//! Internet, else a table of Apple's apps, else what the app says it is
//! (`LSApplicationCategoryType`), else Apple's own apps that say nothing are
//! Utilities (Disk Utility and its like), else Other.

use super::apps::App;
use crate::common::config::{HomeCategory, HomeSettings};

pub fn category(app: &App, settings: &HomeSettings) -> HomeCategory {
    if let Some(category) = settings.category_of(&app.bundle_id) {
        return category;
    }
    if app.browser || web_app(&app.bundle_id) {
        return HomeCategory::Internet;
    }
    if let Some(category) = known(&app.bundle_id) {
        return category;
    }
    if let Some(category) = app.category_type.as_deref().and_then(from_type) {
        return category;
    }
    if app.bundle_id.starts_with("com.apple.") {
        return HomeCategory::Utilities;
    }
    HomeCategory::Other
}

/// A site installed as an app by a Chromium browser (`com.google.Chrome.app.…`).
fn web_app(bundle_id: &str) -> bool {
    [
        "com.google.Chrome.app.",
        "com.brave.Browser.app.",
        "com.microsoft.edgemac.app.",
    ]
    .iter()
    .any(|prefix| bundle_id.starts_with(prefix))
}

/// Apple's app categories (`public.app-category.…`), gathered into home's.
fn from_type(kind: &str) -> Option<HomeCategory> {
    use HomeCategory::*;
    let kind = kind.strip_prefix("public.app-category.")?;
    Some(match kind {
        "social-networking" => Social,
        "productivity" | "business" | "finance" => Productivity,
        "developer-tools" => DeveloperTools,
        "graphics-design" | "photography" => Creativity,
        "entertainment" | "music" | "video" | "sports" | "lifestyle" => Entertainment,
        "games" => Games,
        kind if kind.ends_with("-games") => Games,
        "news"
        | "books"
        | "reference"
        | "education"
        | "travel"
        | "weather"
        | "navigation"
        | "healthcare-fitness"
        | "medical"
        | "food-and-drink"
        | "magazines-and-newspapers" => Information,
        "utilities" => Utilities,
        _ => return None,
    })
}

/// Apple's apps whose own category is missing, or too wide to place them
/// (Mail and FaceTime say "productivity" and "utilities").
fn known(bundle_id: &str) -> Option<HomeCategory> {
    use HomeCategory::*;
    const KNOWN: &[(&str, HomeCategory)] = &[
        ("com.apple.mail", Social),
        ("com.apple.MobileSMS", Social),
        ("com.apple.FaceTime", Social),
        ("com.apple.mobilephone", Social),
        ("com.apple.AddressBook", Social),
        ("com.apple.podcasts", Entertainment),
        ("com.apple.TV", Entertainment),
        ("com.apple.DVDPlayer", Entertainment),
        ("com.apple.Music", Entertainment),
        ("com.apple.weather", Information),
        ("com.apple.stocks", Information),
        ("com.apple.news", Information),
        ("com.apple.Maps", Information),
        ("com.apple.calculator", Utilities),
        ("com.apple.clock", Utilities),
        ("com.apple.findmy", Utilities),
        ("com.apple.Home", Utilities),
        ("com.apple.ScreenContinuity", Utilities),
        ("com.apple.Passwords", Utilities),
        ("com.apple.campo", Utilities),
        ("com.apple.siri.launcher", Utilities),
        ("com.apple.finder", Utilities),
        ("com.apple.Terminal", DeveloperTools),
        ("com.apple.ScriptEditor2", DeveloperTools),
    ];
    KNOWN
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(bundle_id))
        .map(|(_, category)| *category)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(bundle_id: &str, kind: Option<&str>, browser: bool) -> App {
        App {
            bundle_id: bundle_id.into(),
            path: "/Applications/X.app".into(),
            name: "X".into(),
            other_names: Vec::new(),
            category_type: kind.map(|kind| format!("public.app-category.{kind}")),
            browser,
        }
    }

    #[test]
    fn by_hand_then_browser_then_apple_s_table_then_the_app_s_own() {
        let mut settings = HomeSettings::default();
        let category = |app: &App, settings: &HomeSettings| category(app, settings);
        assert_eq!(
            category(&app("dev.zed.Zed", None, false), &settings),
            HomeCategory::Other
        );
        settings.categories.insert("dev.zed.Zed".into(), HomeCategory::DeveloperTools);
        assert_eq!(
            category(&app("dev.zed.Zed", None, false), &settings),
            HomeCategory::DeveloperTools
        );
        assert_eq!(
            category(&app("com.google.Chrome", None, true), &settings),
            HomeCategory::Internet
        );
        assert_eq!(
            category(&app("com.apple.mail", Some("productivity"), false), &settings),
            HomeCategory::Social
        );
        assert_eq!(
            category(&app("dev.x", Some("word-games"), false), &settings),
            HomeCategory::Games
        );
        assert_eq!(
            category(&app("dev.x", Some("something-new"), false), &settings),
            HomeCategory::Other
        );
        assert_eq!(
            category(&app("com.apple.DiskUtility", None, false), &settings),
            HomeCategory::Utilities
        );
        assert_eq!(
            category(&app("com.google.Chrome.app.abc", None, false), &settings),
            HomeCategory::Internet
        );
    }
}

//! Home, `[settings.grid.home]`, as subline's `[home]`: the app grid opened
//! with the `home` command (docs/home.md).

use serde::{Deserialize, Serialize};

use super::{ConfigEnum, ConfigSchema};

fn yes() -> bool { true }
fn default_width() -> f64 { 0.6 }
fn default_height() -> f64 { 0.7 }

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, ConfigSchema)]
#[serde(deny_unknown_fields)]
pub struct HomeSettings {
    #[serde(default)]
    /// How home covers the screen: the whole display, or a panel in its middle
    /// with the windows darkened around it.
    #[setting(
        label = "Style",
        choices,
        order = 0,
        aliases = "launcher full window panel"
    )]
    pub style: HomeStyle,
    #[serde(default = "default_width")]
    /// The panel's width, a share of the display's usable area.
    #[setting(
        label = "Panel width",
        unit = "%",
        scale = 100.0,
        aliases = "launcher size"
    )]
    pub window_width: f64,
    #[serde(default = "default_height")]
    /// The panel's height, a share of the display's usable area.
    #[setting(
        label = "Panel height",
        unit = "%",
        scale = 100.0,
        aliases = "launcher size"
    )]
    pub window_height: f64,
    #[serde(default = "yes")]
    /// With no search, a first row of the apps opened most lately from home.
    #[setting(
        label = "Frequent apps",
        order = 1,
        aliases = "launcher recent most used top"
    )]
    pub frequent: bool,
    #[serde(default = "yes")]
    /// With no search, the grid in boxes by kind of app: Internet,
    /// Productivity, Developer Tools and so on, from what each app says it is.
    #[setting(
        label = "Group apps by category",
        order = 2,
        aliases = "categories categorize kind"
    )]
    pub categorize: bool,
    #[serde(default = "yes")]
    /// Cmd + Space and the four-finger pinch open home instead of Spotlight
    /// and Apps. Off, they stay macOS's.
    #[setting(
        label = "Open home in place of Spotlight",
        order = 3,
        aliases = "spotlight cmd space command pinch launchpad apps"
    )]
    pub take_spotlight: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Bundle ids left out of home's grid and search.
    #[setting(ignore)]
    pub hidden: Vec<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    /// Categories given by hand, by bundle id, over the ones worked out.
    #[setting(ignore)]
    pub categories: std::collections::BTreeMap<String, HomeCategory>,
}

impl Default for HomeSettings {
    fn default() -> Self {
        Self {
            style: HomeStyle::default(),
            window_width: default_width(),
            window_height: default_height(),
            frequent: true,
            categorize: true,
            take_spotlight: true,
            hidden: Vec::new(),
            categories: Default::default(),
        }
    }
}

impl HomeSettings {
    /// The panel's share of the usable area, each way, kept from 0.3 to 1.
    pub fn panel_share(&self) -> (f64, f64) {
        let keep = |v: f64| {
            if v.is_finite() {
                v.clamp(0.3, 1.0)
            } else {
                0.6
            }
        };
        (keep(self.window_width), keep(self.window_height))
    }

    /// The category given by hand to `bundle_id` (case ignored).
    pub fn category_of(&self, bundle_id: &str) -> Option<HomeCategory> {
        self.categories
            .iter()
            .find(|(id, _)| id.eq_ignore_ascii_case(bundle_id))
            .map(|(_, category)| *category)
    }

    /// Whether `bundle_id` is left out (case ignored).
    pub fn hides(&self, bundle_id: &str) -> bool {
        self.hidden.iter().any(|id| id.eq_ignore_ascii_case(bundle_id))
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default, ConfigEnum)]
#[serde(rename_all = "snake_case")]
pub enum HomeStyle {
    /// The whole display, blurred behind, with the clock.
    #[default]
    #[setting(label = "Full screen")]
    Full,
    /// A panel in the middle, the windows around it darkened.
    #[setting(label = "Panel")]
    Window,
}

/// Spotlight's key, which home takes while `take_spotlight` is on.
pub const SPOTLIGHT_KEY: &str = "Cmd + Space";

/// With `take_spotlight`, bind Cmd + Space to `home` in the default keymap,
/// unless the keymap binds it already. `hotkey` is the key as parsed.
pub(super) fn add_spotlight_key(
    home: &HomeSettings,
    hotkey: Option<(String, crate::sys::hotkey::Hotkey)>,
    keys: &mut Vec<(
        crate::sys::hotkey::Hotkey,
        crate::actor::wm_controller::WmCommand,
    )>,
    specs: &mut super::BindingModeSpecs,
) {
    use crate::actor::reactor::Command;
    use crate::actor::wm_controller::WmCommand;
    let (Some((spec, hotkey)), true) = (hotkey, home.take_spotlight) else {
        return;
    };
    let taken = |other: &crate::sys::hotkey::Hotkey| {
        other.key_code == hotkey.key_code
            && other
                .modifiers
                .expand_to_specific()
                .iter()
                .any(|m| hotkey.modifiers.expand_to_specific().contains(m))
    };
    if keys.iter().any(|(other, _)| taken(other)) {
        return;
    }
    let home = WmCommand::ReactorCommand(Command::Reactor(rift_protocol::ReactorCommand::Home));
    keys.push((hotkey, home.clone()));
    if let Some((_, default)) = specs.iter_mut().find(|(mode, _)| mode == "default") {
        default.push((spec, home));
    }
}

/// The kinds of app home groups the grid by, in the grid's order.
#[derive(
    Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, ConfigEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum HomeCategory {
    #[setting(label = "Internet")]
    Internet,
    #[setting(label = "Social")]
    Social,
    #[setting(label = "Productivity")]
    Productivity,
    #[setting(label = "Developer Tools")]
    DeveloperTools,
    #[setting(label = "Creativity")]
    Creativity,
    #[setting(label = "Entertainment")]
    Entertainment,
    #[setting(label = "Games")]
    Games,
    #[setting(label = "Information & Reading")]
    Information,
    #[setting(label = "Utilities")]
    Utilities,
    #[setting(label = "Other")]
    Other,
}

impl HomeCategory {
    pub const ALL: [Self; 10] = [
        Self::Internet,
        Self::Social,
        Self::Productivity,
        Self::DeveloperTools,
        Self::Creativity,
        Self::Entertainment,
        Self::Games,
        Self::Information,
        Self::Utilities,
        Self::Other,
    ];

    pub fn label(self) -> &'static str { Self::CONFIG_CHOICES[self.config_choice_index()].0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_subline_s() {
        let home: HomeSettings = toml::from_str("").unwrap();
        assert_eq!(home, HomeSettings::default());
        assert_eq!(home.style, HomeStyle::Full);
        assert_eq!(home.panel_share(), (0.6, 0.7));
        assert!(home.frequent);
        assert!(home.categorize);
    }

    #[test]
    fn categories_given_by_hand_read_by_bundle_id() {
        let home: HomeSettings =
            toml::from_str("[categories]\n\"dev.zed.Zed\" = \"developer_tools\"\n").unwrap();
        assert_eq!(
            home.category_of("DEV.ZED.ZED"),
            Some(HomeCategory::DeveloperTools)
        );
        assert_eq!(home.category_of("dev.other"), None);
        assert_eq!(HomeCategory::Information.label(), "Information & Reading");
    }

    #[test]
    fn reads_what_is_written() {
        let home: HomeSettings = toml::from_str(
            r#"style = "window"
window_width = 0.1
window_height = 2.0
frequent = false
hidden = ["com.apple.Chess"]"#,
        )
        .unwrap();
        assert_eq!(home.style, HomeStyle::Window);
        assert_eq!(home.panel_share(), (0.3, 1.0));
        assert!(!home.frequent);
        assert!(home.hides("com.apple.chess"));
        assert!(!home.hides("com.apple.Safari"));
    }

    #[test]
    fn home_is_a_key_s_command() {
        use crate::actor::reactor::Command;
        use crate::actor::wm_controller::WmCommand;
        let config =
            super::super::Config::parse("[settings]\n[keys]\n\"Alt + Space\" = \"home\"\n")
                .unwrap();
        let home = WmCommand::ReactorCommand(Command::Reactor(rift_protocol::ReactorCommand::Home));
        assert!(
            config.keys.iter().any(|(_, command)| *command == home),
            "{:?}",
            config.keys
        );
    }

    #[test]
    fn spotlight_s_key_opens_home_unless_it_is_bound_or_given_back() {
        use crate::actor::reactor::Command;
        use crate::actor::wm_controller::WmCommand;
        let home = WmCommand::ReactorCommand(Command::Reactor(rift_protocol::ReactorCommand::Home));
        let parse = |text: &str| super::super::Config::parse(text).unwrap();
        let has = |config: &super::super::Config, command: &WmCommand| {
            config.keys.iter().any(|(_, c)| c == command)
        };
        let config = parse("[settings]\n[keys]\n");
        assert!(has(&config, &home));
        assert!(
            config.binding_mode_specs[0].1.iter().any(|(_, c)| *c == home),
            "the input actor installs the specs"
        );
        let given_back = parse("[settings.grid.home]\ntake_spotlight = false\n[keys]\n");
        assert!(!has(&given_back, &home));
        let bound = parse("[settings]\n[keys]\n\"Cmd + Space\" = \"overview_whole\"\n");
        assert!(!has(&bound, &home), "the keymap's own binding wins");
    }

    #[test]
    fn unknown_keys_are_refused() {
        assert!(toml::from_str::<HomeSettings>("scrim = 0.2").is_err());
    }
}

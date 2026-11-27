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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Bundle ids left out of home's grid and search.
    #[setting(ignore)]
    pub hidden: Vec<String>,
}

impl Default for HomeSettings {
    fn default() -> Self {
        Self {
            style: HomeStyle::default(),
            window_width: default_width(),
            window_height: default_height(),
            frequent: true,
            hidden: Vec::new(),
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
    fn unknown_keys_are_refused() {
        assert!(toml::from_str::<HomeSettings>("scrim = 0.2").is_err());
    }
}

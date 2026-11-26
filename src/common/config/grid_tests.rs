//! The grid fork's settings: grid commands in key bindings, and the grid and
//! border settings in the settings app.

use super::*;
use crate::actor::reactor;
use crate::actor::wm_controller::WmCommand;
use crate::layout_engine::LayoutCommand;

#[test]
fn grid_commands_and_columns_parse() {
    let cfg = Config::parse(
        r#"
            [settings]

            [virtual_workspaces]
            default_workspace_count = 9
            grid_columns = 3

            [keys]
            "Alt + J" = { focus = "left" }
            "Alt + Shift + J" = { move_tile = "left" }
            "Alt + Cmd + J" = { go_to_cell = "left" }
        "#,
    )
    .unwrap();
    assert_eq!(cfg.virtual_workspaces.grid_columns, 3);
    let commands: Vec<_> = cfg.keys.iter().map(|(_, command)| command.clone()).collect();
    for expected in [
        LayoutCommand::Focus(crate::layout_engine::Direction::Left),
        LayoutCommand::MoveTile(crate::layout_engine::Direction::Left),
        LayoutCommand::GoToCell(crate::layout_engine::Direction::Left),
    ] {
        let expected = WmCommand::ReactorCommand(reactor::Command::Layout(expected));
        assert!(commands.contains(&expected), "{expected:?} not in {commands:?}");
    }
}

#[test]
fn grid_settings_are_in_the_settings_app() {
    let columns = VirtualWorkspaceSettings::field("grid_columns").unwrap();
    assert_eq!(columns.title, "Grid columns");
    assert!(matches!(columns.kind, FieldKind::Number { integer: true }));
    use crate::common::config::GridOverviewSettings;
    for (key, unit, scale) in [("gap", "%", 100.0), ("zoom", "%", 100.0)] {
        let field = GridOverviewSettings::field(key).unwrap();
        assert!(matches!(field.kind, FieldKind::Number { integer: false }), "{key}");
        assert_eq!((field.unit, field.scale), (unit, scale), "{key}");
    }
    for key in ["labels", "fit", "full_resolution"] {
        let field = GridOverviewSettings::field(key).unwrap();
        assert!(matches!(field.kind, FieldKind::Bool), "{key}");
    }
    let follow = GridOverviewSettings::field("follow").unwrap();
    let FieldKind::Choice(choices) = follow.kind else {
        panic!("follow is a choice")
    };
    assert_eq!(choices.len(), 2);
    let parsed = Config::parse("[settings.grid.overview]\nfollow = \"center\"\n[keys]\n").unwrap();
    assert_eq!(parsed.settings.grid.overview.follow, crate::common::config::GridFollow::Center);
    let empty: GridOverviewSettings = toml::from_str("").unwrap();
    assert_eq!(empty, GridOverviewSettings::default());
    let mut settings = GridOverviewSettings::default();
    let write = GridOverviewSettings::field("gap").unwrap().write.unwrap();
    write(&mut settings, FieldValue::Number(0.06)).unwrap();
    assert_eq!(settings.gap, 0.06);
}

#[test]
fn grid_columns_and_rule_grids_are_checked() {
    let issues = |toml: &str| Config::parse(toml).unwrap().validate();
    let base = "[settings]\n[keys]\n[virtual_workspaces]\ngrid_columns = 3\n";
    assert!(issues(base).is_empty(), "{:?}", issues(base));
    let wide = issues(&base.replace("= 3", "= 4"));
    assert!(wide.iter().any(|i| i.contains("grid_columns")), "{wide:?}");
    let rule = |grid| {
        issues(&format!(
            "{base}[[virtual_workspaces.app_rules]]\napp_id = \"com.example.a\"\ngrid = {grid}\n"
        ))
    };
    assert!(rule(9).is_empty(), "{:?}", rule(9));
    for grid in [0, 10] {
        assert!(rule(grid).iter().any(|i| i.contains("grid must be")), "grid {grid}");
    }
}

#[test]
fn motion_level_shortens_or_stops_moves() {
    use crate::common::config::{GridMotionSettings, MotionLevel};
    let moves = GridMotionSettings { zoom: 0.15, ..GridMotionSettings::default() };
    let parsed = Config::parse("[settings.grid]\nmotion = \"reduced\"\n[keys]\n").unwrap();
    assert_eq!(parsed.settings.grid.motion, MotionLevel::Reduced);
    assert_eq!(Config::parse("[settings]\n[keys]\n").unwrap().settings.grid.motion, MotionLevel::System);

    assert_eq!(MotionLevel::Full.moves(&moves, true), moves, "full ignores the system");
    assert_eq!(MotionLevel::System.moves(&moves, false), moves);
    let reduced = MotionLevel::System.moves(&moves, true);
    assert_eq!(reduced, MotionLevel::Reduced.moves(&moves, false));
    assert_eq!((reduced.duration_ms, reduced.zoom, reduced.pan), (80.0, 0.0, true));
    let off = MotionLevel::Off.moves(&moves, false);
    assert!(!off.pan && off.map, "moves show at once, the map stays");

    assert_eq!(MotionLevel::Reduced.duration_ms(180.0, false), 60.0);
    assert_eq!(MotionLevel::Off.duration_ms(180.0, false), 0.0);
    assert_eq!(MotionLevel::System.duration_ms(180.0, false), 180.0);
}

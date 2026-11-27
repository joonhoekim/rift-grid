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
            [settings.grid]
            rows = 3
            columns = 5

            [virtual_workspaces]
            default_workspace_count = 9

            [keys]
            "Alt + J" = { focus = "left" }
            "Alt + Shift + J" = { move_tile = "left" }
            "Alt + Cmd + J" = { go_to_cell = "left" }
        "#,
    )
    .unwrap();
    assert_eq!(cfg.virtual_workspaces.grid_shape, Some((3, 5)), "the store reads the shape");
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
    for (key, title) in [("rows", "Rows"), ("columns", "Columns")] {
        let field = GridSettings::field(key).unwrap();
        assert_eq!(field.title, title);
        assert!(matches!(field.kind, FieldKind::Number { integer: true }), "{key}");
    }
    assert!(VirtualWorkspaceSettings::field("grid_shape").is_none(), "not a setting of its own");
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
fn grid_shapes_and_rule_grids_are_checked() {
    let issues = |toml: &str| Config::parse(toml).unwrap().validate();
    let shape = |rows, columns| {
        Config::parse(&format!("[settings.grid]\nrows = {rows}\ncolumns = {columns}\n[keys]\n"))
            .unwrap()
            .virtual_workspaces
            .grid_shape
    };
    assert_eq!(shape(3, 3), Some((3, 3)));
    assert_eq!(shape(1, 7), Some((1, 7)));
    assert_eq!(shape(4, 3), Some((5, 3)), "even reads as the next odd");
    assert_eq!(shape(12, 3), Some((9, 3)), "at most nine");
    assert_eq!(shape(0, 0), None);
    assert_eq!(shape(3, 0), None);
    let grid = |text: &str| issues(&format!("[settings.grid]\n{text}\n[keys]\n"));
    assert!(grid("rows = 3\ncolumns = 5").is_empty());
    assert!(grid("rows = 4\ncolumns = 3").is_empty(), "an even number reads as the next odd");
    assert!(grid("rows = 3").iter().any(|i| i.contains("both rows and columns")));
    assert!(grid("rows = 11\ncolumns = 3").iter().any(|i| i.contains("at most 9")));
    let base = "[settings.grid]\nrows = 3\ncolumns = 3\n[keys]\n";
    assert!(issues(base).is_empty(), "{:?}", issues(base));
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

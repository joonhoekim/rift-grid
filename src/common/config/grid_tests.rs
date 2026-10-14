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
            "Alt + J" = { grid_focus = "left" }
            "Alt + Shift + J" = { grid_move = "left" }
            "Alt + Cmd + J" = { grid_cell = "left" }
        "#,
    )
    .unwrap();
    assert_eq!(cfg.virtual_workspaces.grid_columns, 3);
    let commands: Vec<_> = cfg.keys.iter().map(|(_, command)| command.clone()).collect();
    for expected in [
        LayoutCommand::GridFocus(crate::layout_engine::Direction::Left),
        LayoutCommand::GridMove(crate::layout_engine::Direction::Left),
        LayoutCommand::GridCell(crate::layout_engine::Direction::Left),
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
    for (key, unit, scale) in [
        ("grid_gap", "%", 100.0),
        ("grid_zoom", "%", 100.0),
        ("live_preview_interval_ms", "ms", 1.0),
    ] {
        let field = MissionControlSettings::field(key).unwrap();
        assert!(matches!(field.kind, FieldKind::Number { integer: false }), "{key}");
        assert_eq!((field.unit, field.scale), (unit, scale), "{key}");
    }
    for key in ["grid_labels", "grid_fit"] {
        let field = MissionControlSettings::field(key).unwrap();
        assert!(matches!(field.kind, FieldKind::Bool), "{key}");
    }
    let mut settings = MissionControlSettings::default();
    let write = MissionControlSettings::field("grid_gap").unwrap().write.unwrap();
    write(&mut settings, FieldValue::Number(0.06)).unwrap();
    assert_eq!(settings.grid_gap, 0.06);
}

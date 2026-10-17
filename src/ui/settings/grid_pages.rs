//! The settings app's pages for the grid fork: the grid Overview with how moves
//! between cells are shown, and the window borders.

use super::*;
use crate::common::config::*;

// Replaced by `overview` below; named here so it is not dead code.
const _: fn(Ui, &Rc<Model>) -> Page = super::interface::overview;

/// Upstream's Overview page (`interface::overview`), with the grid's settings.
pub(super) fn overview(ui: Ui, model: &Rc<Model>) -> Page {
    let mut f = FormBuilder::new(ui, model);
    f.tooltips = true;
    let visibility = f.fields(
        Section::new(&ui, "Workspaces"),
        &["enabled", "show_empty_workspaces", "window_previews", "live_preview_interval_ms"],
        |s| &s.settings.ui.mission_control,
        |s| &mut s.settings.ui.mission_control,
    );
    let transitions = f.fields(
        Section::new(&ui, "Transitions"),
        &["fade_enabled"],
        |s| &s.settings.ui.mission_control,
        |s| &mut s.settings.ui.mission_control,
    );
    let duration = f.field(
        "fade_duration_ms",
        |s| &s.settings.ui.mission_control,
        |s| &mut s.settings.ui.mission_control,
    );
    f.enabled(&duration, |s| {
        s.settings.ui.mission_control.enabled && s.settings.ui.mission_control.fade_enabled
    });
    let grid = f.fields(
        Section::new(&ui, "Grid").description("When workspaces form a grid."),
        &["grid_style", "grid_fit", "grid_zoom", "grid_follow", "grid_gap", "grid_labels"],
        |s| &s.settings.ui.mission_control,
        |s| &mut s.settings.ui.mission_control,
    );
    let motion = f.schema_rows(
        Section::new(&ui, "Grid Motion").description(
            "How moving to another cell or grid is shown: the screen slides there, and a \
             small map of the grid shows where you went.",
        ),
        "",
        |s| &s.settings.ui.grid_motion,
        |s| &mut s.settings.ui.grid_motion,
    );
    f.finish(
        SettingsPage::new(&ui, "")
            .section(visibility)
            .section(transitions.row(duration))
            .section(grid)
            .section(motion)
            .section(Caption::new(
                &ui,
                "Turning Overview on or off takes effect after restarting Rift; the rest \
                 applies the next time it opens.",
            )),
    )
}

/// A color well bound to a border color string.
fn border_color(
    f: &mut FormBuilder,
    title: &str,
    get: fn(&ConfigSource) -> &String,
    set: fn(&mut ConfigSource, String),
) -> SettingsRow {
    let message = Rc::new(ValidationMessage::new(&f.ui));
    let commit = f.assign(
        move |s| get(s).clone(),
        move |s, value| set(s, value),
        Rc::downgrade(&message),
    );
    let well = Rc::new(
        ColorWell::new(&f.ui)
            .on_change(move |rgba| commit(crate::common::config::borders::format_color(rgba))),
    );
    f.sync(&well, move |well, s| {
        if let Some(rgba) = crate::common::config::borders::parse_color(get(s)) {
            well.set_value(rgba);
        }
    });
    f.row(title, well, message)
}

pub(super) fn borders(ui: Ui, model: &Rc<Model>) -> Page {
    let mut f = FormBuilder::new(ui, model);
    let section = f.schema_rows(
        Section::new(&ui, "Window Borders").description(
            "A ring just outside each window of the active workspaces, the focused one in \
             its own color.",
        ),
        "",
        |s| &s.settings.ui.borders,
        |s| &mut s.settings.ui.borders,
    );
    let focused = border_color(
        &mut f,
        "Focused color",
        |s| &s.settings.ui.borders.active_color,
        |s, v| s.settings.ui.borders.active_color = v,
    );
    let unfocused = border_color(
        &mut f,
        "Unfocused color",
        |s| &s.settings.ui.borders.inactive_color,
        |s, v| s.settings.ui.borders.inactive_color = v,
    );
    let message = Rc::new(ValidationMessage::new(&ui));
    let commit = f.assign(
        |s| !s.settings.ui.borders.inactive_color.trim().is_empty(),
        |s, on| {
            s.settings.ui.borders.inactive_color = if on {
                BorderSettings::default().inactive_color
            } else {
                String::new()
            }
        },
        Rc::downgrade(&message),
    );
    let switch = Rc::new(Switch::new(&ui).on_change(commit));
    f.sync(&switch, |switch, s| {
        switch.set_value(!s.settings.ui.borders.inactive_color.trim().is_empty())
    });
    let bare = f.row("Unfocused borders", switch, message);
    let colors = Section::new(&ui, "Colors")
        .description(
            "Colors are kept in the config file, where other tools, such as one that \
             follows the wallpaper, may set them too.",
        )
        .row(focused)
        .row(bare)
        .row(unfocused);
    f.finish(SettingsPage::new(&ui, "").section(section).section(colors))
}

//! The settings app's pages for the grid fork: the grid sections of the Overview, how moves
//! between cells are shown, and the window borders.

use cgs::controls::ColorWell;
use cgs::settings::{Section, SettingsPage};

use super::*;
use crate::common::config::*;

/// The grid's sections of upstream's Overview page (`interface::overview`).
pub(super) fn overview_sections(f: &mut FormBuilder) -> (Section, Section) {
    let ui = f.ui;
    let grid = f.fields(
        Section::new(&ui, "Grid").description("When workspaces form a grid."),
        &[
            "grid_fit",
            "grid_zoom",
            "grid_follow",
            "grid_gap",
            "grid_labels",
        ],
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
    (grid, motion)
}

/// The grid's swipes, on the Mouse & Trackpad page after upstream's.
pub(super) fn swipes(f: &mut FormBuilder) -> Section {
    let ui = f.ui;
    f.schema_rows(
        Section::new(&ui, "Grid swipes").description(
            "While workspaces form a grid. Distance, tolerance and haptics are the \
             workspace swipes' own.",
        ),
        "",
        |s| &s.settings.grid_gestures,
        |s| &mut s.settings.grid_gestures,
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
    // `ColorWell` turns continuous off, and then a choice in the color panel
    // never sends the action. Like the sliders, commit as the color changes.
    well.ns_color_well().setContinuous(true);
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

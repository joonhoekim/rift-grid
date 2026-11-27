//! The settings app's pages for the grid fork, as subline's: Grid, Cells and
//! Focus, and the grid's sections on upstream's Overview, Mouse & Trackpad and
//! Menu Bar pages.

use cgs::controls::ColorWell;
use cgs::settings::{Section, SettingsPage};

use super::*;
use crate::common::config::*;

/// The Grid page: the grids' size and where a display starts.
pub(super) fn grid(ui: Ui, model: &Rc<Model>) -> Page {
    let mut f = FormBuilder::new(ui, model);
    f.tooltips = true;
    let grid = f.schema_rows(
        Section::new(&ui, "Grid").description(
            "Nine grids of cells, one per number key. Each cell is a workspace. Rows and \
             columns take effect when rift-grid starts again; cells are named in the \
             config file ([settings.grid] names, names_2 to names_9), and upstream's \
             default workspace and workspace names are not used.",
        ),
        "",
        |s| &s.settings.grid,
        |s| &mut s.settings.grid,
    );
    f.finish(SettingsPage::new(&ui, "").section(grid))
}

/// The Cells page: how moves between cells are shown, and how much they move.
pub(super) fn cells(ui: Ui, model: &Rc<Model>) -> Page {
    let mut f = FormBuilder::new(ui, model);
    f.tooltips = true;
    let moves = f.schema_rows(
        Section::new(&ui, "Showing moves").description(
            "How moving to another cell or grid is shown: the screen slides there, and a \
             small map of the grid shows where you went.",
        ),
        "",
        |s| &s.settings.grid.moves,
        |s| &mut s.settings.grid.moves,
    );
    let motion = f.schema_rows(
        Section::new(&ui, "Accessibility"),
        "cells",
        |s| &s.settings.grid,
        |s| &mut s.settings.grid,
    );
    f.finish(SettingsPage::new(&ui, "").section(moves).section(motion))
}

/// The grid's section on upstream's Overview page, after its transitions.
pub(super) fn overview_section(f: &mut FormBuilder, ui: &Ui) -> Section {
    f.schema_rows(
        Section::new(ui, "Grid").description("How the Overview shows a grid of cells."),
        "",
        |s| &s.settings.grid.overview,
        |s| &mut s.settings.grid.overview,
    )
}

/// The grid's swipes, on upstream's Mouse & Trackpad page.
pub(super) fn swipes_section(f: &mut FormBuilder, ui: &Ui) -> Section {
    f.schema_rows(
        Section::new(ui, "Grid").description(
            "Distance, tolerance and haptics are the workspace swipes' own, above.",
        ),
        "",
        |s| &s.settings.grid.gestures,
        |s| &mut s.settings.grid.gestures,
    )
}

/// The grid map, on upstream's Menu Bar page.
pub(super) fn menu_bar_section(f: &mut FormBuilder, ui: &Ui) -> Section {
    f.schema_rows(
        Section::new(ui, "Grid"),
        "menu_bar",
        |s| &s.settings.grid,
        |s| &mut s.settings.grid,
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
        Section::new(&ui, "Focus line").description(
            "A ring just outside each window of the active workspaces, the focused one in \
             its own color.",
        ),
        "",
        |s| &s.settings.grid.focus,
        |s| &mut s.settings.grid.focus,
    );
    let focused = border_color(
        &mut f,
        "Border color",
        |s| &s.settings.grid.focus.border_color,
        |s, v| s.settings.grid.focus.border_color = v,
    );
    let unfocused = border_color(
        &mut f,
        "Unfocused color",
        |s| &s.settings.grid.focus.unfocused_color,
        |s, v| s.settings.grid.focus.unfocused_color = v,
    );
    let message = Rc::new(ValidationMessage::new(&ui));
    let commit = f.assign(
        |s| !s.settings.grid.focus.unfocused_color.trim().is_empty(),
        |s, on| {
            s.settings.grid.focus.unfocused_color = if on {
                BorderSettings::default().unfocused_color
            } else {
                String::new()
            }
        },
        Rc::downgrade(&message),
    );
    let switch = Rc::new(Switch::new(&ui).on_change(commit));
    f.sync(&switch, |switch, s| {
        switch.set_value(!s.settings.grid.focus.unfocused_color.trim().is_empty())
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

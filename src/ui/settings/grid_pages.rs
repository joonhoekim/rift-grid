//! The settings app's pages for the grid fork, as subline's: Grid, Cells and
//! Focus, and the grid's sections on upstream's Overview, Mouse & Trackpad and
//! Menu Bar pages.

use cgs::controls::ColorWell;
use cgs::settings::{Section, SettingsPage};

use super::*;
use crate::common::config::*;
use crate::layout_engine::Direction;

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

/// The grid's swipes, on upstream's Mouse & Trackpad page: subline's table, a
/// row for each way and a column for three and four fingers.
pub(super) fn swipes_section(f: &mut FormBuilder, ui: &Ui) -> Section {
    let mut section = Section::new(ui, "Grid").description(
        "What each swipe does on the grid. The content follows the fingers: swiping left \
         shows what is to the right. In the Overview, the opposite of the swipe that opens \
         it steps back, and out. A swipe that does nothing is left to macOS. Distance and \
         haptics are the workspace swipes', above.",
    );
    let header = HStack::new(ui).spacing(GESTURE_GAP);
    let header = ["Three fingers", "Four fingers"].into_iter().fold(header, |row, title| {
        let label = Caption::single_line(ui, title);
        label.width(GESTURE_COLUMN);
        row.push(label)
    });
    section = section.row(f.row("", header, Rc::new(ValidationMessage::new(ui))));
    for (title, way) in [
        ("Swipe left", Direction::Left),
        ("Swipe right", Direction::Right),
        ("Swipe up", Direction::Up),
        ("Swipe down", Direction::Down),
    ] {
        let message = Rc::new(ValidationMessage::new(ui));
        let row = HStack::new(ui)
            .spacing(GESTURE_GAP)
            .push(gesture_popup(f, false, way, &message))
            .push(gesture_popup(f, true, way, &message));
        section = section.row(f.row(title, row, message));
    }
    section
}

const GESTURE_COLUMN: f64 = 190.0;
const GESTURE_GAP: f64 = 8.0;

/// A popup choosing what the swipe `way` of three fingers, or of four, does.
fn gesture_popup(
    f: &mut FormBuilder,
    four: bool,
    way: Direction,
    message: &Rc<ValidationMessage>,
) -> Rc<Popup> {
    fn hand(s: &ConfigSource, four: bool) -> SwipeActions {
        let gestures = &s.settings.grid.gestures;
        if four { gestures.four } else { gestures.three }
    }
    let get = move |s: &ConfigSource| hand(s, four).get(way);
    let change = f.assign(
        get,
        move |s, action| {
            let gestures = &mut s.settings.grid.gestures;
            if four { &mut gestures.four } else { &mut gestures.three }.set(way, action)
        },
        Rc::downgrade(message),
    );
    let popup = Rc::new(
        Popup::new(&f.ui)
            .items(GestureAction::CONFIG_CHOICES.iter().map(|(label, _)| *label))
            .on_change(move |index| {
                if let Some(action) = GestureAction::from_config_choice(index) {
                    change(action)
                }
            }),
    );
    popup.width(GESTURE_COLUMN);
    popup.set_item_badge(get(defaults()).config_choice_index(), Some("Default"));
    f.sync_when(&popup, get, move |popup, s| popup.set_selected(get(s).config_choice_index()));
    popup
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

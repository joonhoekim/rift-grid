//! The settings app's pages for the grid fork: the grid Overview and the window
//! borders.

use super::*;
use crate::common::config::*;

pub(super) fn overview(ui: Ui, model: &Rc<Model>) -> Page {
    let mut f = FormBuilder::new(ui, model);
    let section = f.schema_rows(
        Section::new(&ui, "Overview").description(
            "Turning Overview on or off requires restarting Rift. The rest applies the next \
             time it opens; the grid settings shape it when workspaces form a grid.",
        ),
        "",
        |s| &s.settings.ui.mission_control,
        |s| &mut s.settings.ui.mission_control,
    );
    f.finish(SettingsPage::new(&ui, "").section(section))
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
            "apps/rice/colors may set these from the wallpaper; it leaves colors you \
             picked here alone.",
        )
        .row(focused)
        .row(bare)
        .row(unfocused);
    f.finish(SettingsPage::new(&ui, "").section(section).section(colors))
}

//! A shortcut row's keys: one recorder for each key bound to its command, and one more
//! that records another key for it.
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use cgs::controls::Button;
use cgs::input::KeyRecorder;
use cgs::layout::{Divider, GroupBox, HStack, VStack, uniform_insets};
use cgs::menu::Menu;
use cgs::settings::{ShortcutPresentation as Style, ShortcutRow};
use cgs::text::Label;
use cgs::view::{HoverReveal, NativeView, View};
use cgs::{Insets, Ui};
use objc2::Message;
use objc2_app_kit::{NSColor, NSLayoutConstraintOrientation};

/// Every key's width, which fits four modifiers and a key, such as ⌃⌥⇧⌘← or ⌃⌥⇧Space,
/// so keys and argument popups line up from row to row.
const KEY_WIDTH: f64 = 100.0;
const KEY_SPACING: f64 = 6.0;

pub(super) struct KeyChips {
    ui: Ui,
    /// The keys, left-aligned in room for at least two; more grow the row to the left.
    view: HStack,
    /// Holds the second key's place in a row showing only one.
    filler: Rc<View>,
    /// Each key's recorder, with its menu, which must outlive the field showing it.
    keys: RefCell<Vec<(Rc<KeyRecorder>, Menu)>>,
    /// The recorder for another key, shown while it records or when there are no keys.
    another: RefCell<Option<(Rc<KeyRecorder>, Rc<GroupBox>)>>,
}

impl KeyChips {
    pub(super) fn new(ui: &Ui) -> Rc<Self> {
        let filler = Rc::new(View::new(ui));
        filler.width(KEY_WIDTH);
        Rc::new(Self {
            ui: *ui,
            view: HStack::new(ui).spacing(KEY_SPACING),
            filler,
            keys: RefCell::new(Vec::new()),
            another: RefCell::new(None),
        })
    }

    fn field(&self, recorder: &Rc<KeyRecorder>) -> Rc<GroupBox> {
        let field = Rc::new(recorder.flat_field(&self.ui));
        field.width(KEY_WIDTH);
        field
    }

    /// Show a bound key; `menu` is its context menu.
    pub(super) fn push(&self, recorder: &Rc<KeyRecorder>, menu: Menu) {
        let field = self.field(recorder);
        unsafe { field.ns_view().setMenu(Some(menu.ns_menu())) };
        self.view.add(field);
        self.keys.borrow_mut().push((recorder.clone(), menu));
    }

    /// The recorder that adds a key, last; it stays visible when the command has no keys.
    pub(super) fn set_another(&self, recorder: &Rc<KeyRecorder>, visible: bool) {
        let field = self.field(recorder);
        field.set_hidden(!visible);
        self.view.add(field.clone());
        self.view.add(self.filler.clone());
        *self.another.borrow_mut() = Some((recorder.clone(), field));
        self.fill();
    }

    fn fill(&self) {
        let another = self
            .another
            .borrow()
            .as_ref()
            .is_some_and(|(_, field)| !field.ns_view().isHidden());
        self.filler.set_hidden(self.keys.borrow().len() + usize::from(another) != 1);
    }

    /// Record another key for the command.
    pub(super) fn add_another(&self) {
        let another = self.another.borrow().clone();
        if let Some((recorder, field)) = another {
            field.set_hidden(false);
            self.fill();
            recorder.begin_recording();
        }
    }

    /// Shows the recorder for another key while it records, and hides it again however
    /// recording ends: a key, Delete, Escape, or focus moving elsewhere.
    pub(super) fn recording(self: &Rc<Self>) -> Rc<dyn Fn(bool)> {
        let chips: Weak<Self> = Rc::downgrade(self);
        Rc::new(move |recording| {
            let Some(chips) = chips.upgrade() else { return };
            if !recording {
                return chips.hide_another();
            }
            if let Some((_, field)) = chips.another.borrow().as_ref() {
                field.set_hidden(false);
            }
            chips.fill();
        })
    }

    fn hide_another(&self) {
        if self.keys.borrow().is_empty() {
            return;
        }
        if let Some((_, field)) = self.another.borrow().as_ref() {
            field.set_hidden(true);
        }
        self.fill();
    }

    pub(super) fn cancel(&self) {
        for (recorder, _) in self.keys.borrow().iter() {
            recorder.cancel();
        }
        let another = self.another.borrow().clone();
        if let Some((recorder, _)) = another {
            recorder.cancel();
        }
        self.hide_another();
    }
}

impl NativeView for KeyChips {
    fn ns_view(&self) -> &objc2_app_kit::NSView { self.view.ns_view() }
}

/// `ShortcutRow::new`, with a row of keys in place of its single recorder and the add
/// button after them.
#[allow(clippy::too_many_arguments)]
pub(super) fn shortcut_row(
    ui: &Ui,
    name: Rc<Label>,
    parameter: Option<Box<dyn NativeView>>,
    keys: Rc<KeyChips>,
    another: Button,
    parameter_error: impl NativeView,
    error: impl NativeView,
    conflicts: impl NativeView,
) -> ShortcutRow {
    Style::flexible(&*name);
    another.ns_button().setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    another.width(14.0);
    another.height(20.0);
    let action = another.ns_view().retain();
    let slot = View::new(ui).content(another, uniform_insets(0.0));
    slot.width(14.0);
    slot.height(20.0);
    let controls = HoverReveal::new(ui, HStack::new(ui).spacing(6.0).push(keys).push(slot), action);
    let line = HStack::new(ui)
        .spacing(12.0)
        .insets(Insets {
            top: 8.0,
            left: 12.0,
            bottom: 8.0,
            right: 12.0,
        })
        .push(name)
        .spacer(ui);
    if let Some(parameter) = parameter {
        parameter.compression_resistance(250.0, NSLayoutConstraintOrientation::Horizontal);
        parameter.max_width(180.0);
        line.add(parameter);
    }
    line.add(controls);
    line.min_height(40.0);
    Style::flexible(&parameter_error);
    Style::flexible(&error);
    let errors = VStack::new(ui)
        .spacing(2.0)
        .insets(Insets {
            top: 0.0,
            left: 12.0,
            bottom: 0.0,
            right: 12.0,
        })
        .push(parameter_error)
        .push(error);
    let divider = Rc::new(
        HStack::new(ui)
            .insets(Insets {
                top: 2.0,
                left: 12.0,
                bottom: 2.0,
                right: 12.0,
            })
            .push(Divider::new(ui)),
    );
    let content = VStack::new(ui)
        .spacing(0.0)
        .push(divider.clone())
        .push(line)
        .push(errors)
        .push(conflicts);
    let view = Rc::new(View::settings_card(ui).content(content, uniform_insets(0.0)));
    let cell = Style::pane_item(ui, view.clone());
    ShortcutRow { view, cell, divider }
}

/// Bindings grouped by command, one entry per command in the order of its first key, with
/// its keys sorted by how they `show`, so ⌥K comes before ⌥↓.
pub(super) fn by_command<'a, C: PartialEq + Clone + 'a>(
    bindings: impl IntoIterator<Item = (&'a String, &'a C)>,
    show: impl Fn(&str) -> String,
) -> Vec<(Vec<String>, C)> {
    let mut groups: Vec<(Vec<String>, C)> = Vec::new();
    for (key, command) in bindings {
        match groups.iter_mut().find(|(_, other)| other == command) {
            Some((keys, _)) => keys.push(key.clone()),
            None => groups.push((vec![key.clone()], command.clone())),
        }
    }
    for (keys, _) in &mut groups {
        keys.sort_by_cached_key(|key| (show(key), key.clone()));
    }
    groups
}

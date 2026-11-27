//! Home: a searchable grid of the installed apps over the current cell, as
//! subline's home (docs/home.md).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub mod apps;
pub mod grid;
pub mod launches;
pub mod panel;
pub mod search;

/// Home's window number, 0 before it is made; read from the reactor's and the
/// input's threads.
static WINDOW: AtomicU32 = AtomicU32::new(0);
static OPEN: AtomicBool = AtomicBool::new(false);

fn set_window(number: u32) { WINDOW.store(number, Ordering::Relaxed); }

fn set_open(open: bool) { OPEN.store(open, Ordering::Relaxed); }

/// Whether home is open.
pub fn is_open() -> bool { OPEN.load(Ordering::Relaxed) }

/// Home's window number, while it is open.
pub fn open_window() -> Option<u32> {
    let number = WINDOW.load(Ordering::Relaxed);
    (is_open() && number != 0).then_some(number)
}

/// Whether `wid` is home's window: the pointer stops at it.
pub fn is_window(wid: u32) -> bool { wid != 0 && WINDOW.load(Ordering::Relaxed) == wid }

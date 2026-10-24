//! rift-grid's names for the things upstream rift also names: the binaries, the
//! Mach service and launchd label, and the directories. They differ from
//! upstream's so that the two never talk to each other's server or read each
//! other's files.

/// The Mach bootstrap service, the launchd label and the bundle identifier.
pub const SERVICE_NAME: &str = "io.github.joonhoekim.rift-grid";
/// The window manager binary.
pub const BINARY: &str = "rift-grid";
/// The command line client.
pub const CLI_BINARY: &str = "rift-grid-cli";
/// The directory under `~/.config`, and with a dot the data directory in `~`.
pub const DIR_NAME: &str = "rift-grid";

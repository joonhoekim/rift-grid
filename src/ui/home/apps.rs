//! The apps home shows: the `.app` bundles in the usual folders, read off the
//! main thread (docs/home.md, "The apps").

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use objc2::runtime::AnyObject;
use objc2_foundation::{NSDictionary, NSFileManager, NSNumber, NSString};

/// How far down folders are read, as upstream's scanner.
const DEPTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub bundle_id: String,
    pub path: PathBuf,
    /// Finder's name: localized ("계산기").
    pub name: String,
    /// The bundle's own names and the file name, to search by too.
    pub other_names: Vec<String>,
}

/// The apps as read, and what to look at to know they are out of date.
#[derive(Debug, Default, Clone)]
pub struct Catalog {
    pub apps: Vec<App>,
    stamps: Vec<(PathBuf, Option<SystemTime>)>,
}

impl Catalog {
    /// Whether a folder read since changed: an app was added, removed or
    /// replaced. A few dozen `lstat`s.
    pub fn stale(&self) -> bool { self.stamps.iter().any(|(path, when)| modified(path) != *when) }

    pub fn find(&self, bundle_id: &str) -> Option<&App> {
        self.apps.iter().find(|app| app.bundle_id.eq_ignore_ascii_case(bundle_id))
    }
}

/// The folders read, in the order that decides which copy of an app wins.
fn roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Library/CoreServices/Applications"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(Path::new(&home).join("Applications"));
    }
    roots
}

/// Finder is in none of the folders.
const FINDER: &str = "/System/Library/CoreServices/Finder.app";

/// Read the installed apps. Safe off the main thread.
pub fn read() -> Catalog {
    objc2::rc::autoreleasepool(|_| {
        let mut catalog = read_from(&roots(), &[PathBuf::from(FINDER)], &display_name);
        sort(&mut catalog.apps);
        catalog
    })
}

/// Finder's name for the bundle, without `.app`.
fn display_name(path: &Path) -> String {
    let manager = NSFileManager::defaultManager();
    let name = manager
        .displayNameAtPath(&NSString::from_str(&path.to_string_lossy()))
        .to_string();
    strip_app(&name).to_owned()
}

fn strip_app(name: &str) -> &str {
    match name.len().checked_sub(4) {
        Some(at) if name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".app") => {
            &name[..at]
        }
        _ => name,
    }
}

/// By name, as Finder sorts.
fn sort(apps: &mut [App]) {
    apps.sort_by(|a, b| {
        let (a, b) = (NSString::from_str(&a.name), NSString::from_str(&b.name));
        a.localizedStandardCompare(&b).cmp(&objc2_foundation::NSComparisonResult::Same)
    });
}

/// `roots`' apps and the `extra` bundles; the first copy of a bundle id wins.
fn read_from(roots: &[PathBuf], extra: &[PathBuf], name_of: &dyn Fn(&Path) -> String) -> Catalog {
    let mut reader = Reader {
        catalog: Catalog::default(),
        seen_dirs: HashSet::new(),
        seen_ids: HashSet::new(),
        name_of,
    };
    for root in roots {
        reader.folder(root, DEPTH);
    }
    for bundle in extra {
        reader.bundle(bundle);
    }
    reader.catalog
}

struct Reader<'a> {
    catalog: Catalog,
    /// Folders read, by their real path, so a link loop ends.
    seen_dirs: HashSet<PathBuf>,
    seen_ids: HashSet<String>,
    name_of: &'a dyn Fn(&Path) -> String,
}

impl Reader<'_> {
    fn folder(&mut self, dir: &Path, depth: usize) {
        // The date of the folder itself, not of what a link points at: a
        // store folder's is 1970, and the folder holding the link changes
        // when the link is replaced.
        self.catalog.stamps.push((dir.to_owned(), modified(dir)));
        let Ok(real) = dir.canonicalize() else { return };
        if !self.seen_dirs.insert(real) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            let hidden =
                path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.'));
            if hidden {
                continue;
            }
            if is_bundle(&path) {
                self.bundle(&path);
            } else if depth > 0 && path.is_dir() {
                // `is_dir` follows links, so linked folders are read too.
                self.folder(&path, depth - 1);
            }
        }
    }

    fn bundle(&mut self, path: &Path) {
        let Some(info) = read_info(path) else { return };
        if info.background_only || !self.seen_ids.insert(info.id.to_lowercase()) {
            return;
        }
        let name = composed(&(self.name_of)(path));
        let file = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let mut other_names = Vec::new();
        let others = [info.display_name, info.name, Some(file)];
        for other in others.into_iter().flatten().map(|other| composed(&other)) {
            if other != name && !other_names.contains(&other) {
                other_names.push(other);
            }
        }
        self.catalog.apps.push(App {
            bundle_id: info.id,
            path: path.to_owned(),
            name,
            other_names,
        });
    }
}

/// File names come decomposed (Hangul as its letters, accents apart); what is
/// typed comes composed. Everything kept is composed, so the two compare.
fn composed(text: &str) -> String {
    NSString::from_str(text).precomposedStringWithCanonicalMapping().to_string()
}

fn is_bundle(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(path).and_then(|meta| meta.modified()).ok()
}

struct Info {
    id: String,
    name: Option<String>,
    display_name: Option<String>,
    background_only: bool,
}

#[allow(deprecated)]
fn read_info(path: &Path) -> Option<Info> {
    let file = NSString::from_str(&path.join("Contents/Info.plist").to_string_lossy());
    let info = unsafe { NSDictionary::<NSString, AnyObject>::dictionaryWithContentsOfFile(&file) }?;
    let value = |key: &str| info.objectForKey(&NSString::from_str(key));
    let text = |key: &str| {
        value(key)
            .and_then(|value| value.downcast::<NSString>().ok())
            .map(|value| value.to_string())
            .filter(|value| !value.trim().is_empty())
    };
    // Written as a boolean, a number, or a string ("1", "YES").
    let flag = |key: &str| match value(key) {
        None => false,
        Some(value) => match value.downcast::<NSNumber>() {
            Ok(number) => number.boolValue(),
            Err(value) => value.downcast::<NSString>().is_ok_and(|text| {
                matches!(
                    text.to_string().trim().to_ascii_lowercase().as_str(),
                    "1" | "yes" | "true"
                )
            }),
        },
    };
    Some(Info {
        id: text("CFBundleIdentifier")?,
        name: text("CFBundleName"),
        display_name: text("CFBundleDisplayName"),
        background_only: flag("LSBackgroundOnly"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, id: &str, extra: &str) {
        let contents = root.join(relative).join("Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            format!(
                r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{id}</string>{extra}</dict></plist>"#
            ),
        )
        .unwrap();
    }

    fn file_name(path: &Path) -> String { path.file_stem().unwrap().to_string_lossy().into_owned() }

    fn ids(catalog: &Catalog) -> Vec<&str> {
        let mut ids: Vec<&str> = catalog.apps.iter().map(|app| app.bundle_id.as_str()).collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn reads_bundles_and_leaves_out_what_cannot_be_opened() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Plain.app", "dev.plain", "");
        write(root, "Utilities/Deep.app", "dev.deep", "");
        write(root, "Menu.app", "dev.menu", "<key>LSUIElement</key><true/>");
        write(
            root,
            "Agent.app",
            "dev.agent",
            "<key>LSBackgroundOnly</key><string>1</string>",
        );
        write(root, ".Hidden.app", "dev.hidden", "");
        write(
            root,
            "Plain.app/Contents/Library/LoginItems/Helper.app",
            "dev.helper",
            "",
        );
        std::fs::create_dir_all(root.join("Broken.app")).unwrap();
        let catalog =
            objc2::rc::autoreleasepool(|_| read_from(&[root.to_owned()], &[], &file_name));
        assert_eq!(ids(&catalog), ["dev.deep", "dev.menu", "dev.plain"]);
    }

    #[test]
    fn the_first_folder_wins_and_links_are_followed() {
        let dir = tempfile::tempdir().unwrap();
        let (first, second, store) = (
            dir.path().join("first"),
            dir.path().join("second"),
            dir.path().join("store"),
        );
        write(&first, "Mail.app", "com.example.mail", "");
        write(&second, "Mail Copy.app", "com.example.MAIL", "");
        write(&store, "apps/Linked.app", "dev.linked", "");
        write(&store, "Folder/InFolder.app", "dev.in-folder", "");
        std::os::unix::fs::symlink(store.join("apps/Linked.app"), second.join("Linked.app"))
            .unwrap();
        std::os::unix::fs::symlink(store.join("Folder"), second.join("Home Manager Apps")).unwrap();
        // A loop: the folder links back to its parent.
        std::os::unix::fs::symlink(&second, second.join("Loop")).unwrap();
        let catalog = objc2::rc::autoreleasepool(|_| {
            read_from(&[first.clone(), second.clone()], &[], &file_name)
        });
        assert_eq!(ids(&catalog), [
            "com.example.mail",
            "dev.in-folder",
            "dev.linked"
        ]);
        assert_eq!(
            catalog.find("COM.EXAMPLE.MAIL").unwrap().path,
            first.join("Mail.app")
        );
    }

    #[test]
    fn other_names_are_the_bundle_s_names_and_the_file_name() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "Calculator.app",
            "com.apple.calculator",
            "<key>CFBundleName</key><string>Calculator</string><key>CFBundleDisplayName</key><string>Calc</string>",
        );
        let catalog = objc2::rc::autoreleasepool(|_| {
            read_from(&[dir.path().to_owned()], &[], &|_| "계산기".to_owned())
        });
        let app = &catalog.apps[0];
        assert_eq!(app.name, "계산기");
        assert_eq!(app.other_names, ["Calc", "Calculator"]);
    }

    #[test]
    fn a_change_in_any_folder_read_makes_it_stale() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Nix Apps/A.app", "dev.a", "");
        let catalog =
            objc2::rc::autoreleasepool(|_| read_from(&[dir.path().to_owned()], &[], &file_name));
        assert!(!catalog.stale());
        // Back-date the folder, then add an app in it: only the inner folder's
        // date moves.
        let inner = dir.path().join("Nix Apps");
        let past = SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::open(&inner).unwrap().set_modified(past).unwrap();
        let catalog =
            objc2::rc::autoreleasepool(|_| read_from(&[dir.path().to_owned()], &[], &file_name));
        assert!(!catalog.stale());
        write(dir.path(), "Nix Apps/B.app", "dev.b", "");
        assert!(catalog.stale());
    }

    #[test]
    fn decomposed_file_names_are_kept_composed() {
        let dir = tempfile::tempdir().unwrap();
        // 밀리 as a file system writes it: its letters apart.
        let decomposed = "\u{1106}\u{1175}\u{11af}\u{1105}\u{1175}";
        write(dir.path(), &format!("{decomposed}.app"), "kr.example.millie", "");
        let catalog = objc2::rc::autoreleasepool(|_| {
            read_from(&[dir.path().to_owned()], &[], &|_| "밀리".to_owned())
        });
        assert_eq!(catalog.apps[0].name, "밀리");
        assert!(catalog.apps[0].other_names.is_empty());
    }

    #[test]
    fn display_names_lose_their_extension() {
        assert_eq!(strip_app("Safari.app"), "Safari");
        assert_eq!(strip_app("계산기"), "계산기");
        assert_eq!(strip_app("app"), "app");
    }
}

#[cfg(test)]
mod this_mac {
    /// `cargo test --lib this_mac -- --ignored --nocapture`: the apps on this Mac.
    #[test]
    #[ignore]
    fn lists_the_apps_here() {
        let started = std::time::Instant::now();
        let catalog = super::read();
        let took = started.elapsed();
        for app in &catalog.apps {
            println!("{:<32} {:<44} {:?}", app.name, app.bundle_id, app.other_names);
        }
        println!(
            "{} apps in {took:?}, {} folders watched",
            catalog.apps.len(),
            catalog.stamps.len()
        );
        let started = std::time::Instant::now();
        assert!(!catalog.stale());
        println!("stale check {:?}", started.elapsed());
    }
}

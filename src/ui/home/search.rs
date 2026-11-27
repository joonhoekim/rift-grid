//! Matching the search against the apps, in subline's order (docs/home.md,
//! "Search").

use super::apps::App;

/// The apps that match `query`, best first, as indices into `apps`. Each word
/// of the search must be in the name or one of the other names, case and
/// accents ignored. In order:
///
/// 1. names that start with the search;
/// 2. names whose initials start with it: the first letter of each word
///    (`bb` for Brave Browser) or the first consonant of each Hangul syllable
///    (`ㅋㅋㅇㅌ` for 카카오톡);
/// 3. names that hold every word;
/// 4. apps found only through their other names;
/// 5. apps found once the search is read as typed in the other keyboard
///    layout, Korean two-set and QWERTY (`초개ㅡㄷ` is `chrome` typed with
///    the Korean layout on, `zkzkdhxhr` is 카카오톡 typed with it off).
///
/// Within each, the order of `apps` (by name).
pub fn search(apps: &[App], query: &str) -> Vec<usize> {
    let query = fold(query.trim());
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    // Initials are matched for a search of one word of two letters or more.
    let initial = (words.len() == 1 && query.chars().count() >= 2).then_some(query.as_str());
    let typed = keys(&query);
    let typed_words: Vec<&str> = typed.split_whitespace().collect();
    let query_hangul = has_hangul(&query);
    let mut tiers: [Vec<usize>; 5] = Default::default();
    for (index, app) in apps.iter().enumerate() {
        let name = fold(&app.name);
        let others: Vec<String> = app.other_names.iter().map(|other| fold(other)).collect();
        let raw = || std::iter::once(&app.name).chain(&app.other_names);
        let names = || std::iter::once(&name).chain(&others);
        let holds = |text: &String, word: &&str| text.contains(word);
        let tier = if name.starts_with(&query) {
            0
        } else if initial.is_some_and(|q| raw().any(|text| fold(&initials(text)).starts_with(q))) {
            1
        } else if words.iter().all(|word| holds(&name, word)) {
            2
        } else if words.iter().all(|word| names().any(|text| holds(text, word))) {
            3
        } else if typed_words.iter().all(|word| {
            // At a word's start, and not Latin against Latin (that is step
            // 3 again): a Hangul search against Latin names, a Latin one
            // against Hangul names, and a Hangul one still being composed
            // (ㄱ, 계ㅅ) against Hangul names.
            names().filter(|text| query_hangul || has_hangul(text)).any(|text| {
                keys(text)
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|key| key.starts_with(word))
            })
        }) {
            4
        } else {
            continue;
        };
        tiers[tier].push(index);
    }
    tiers.concat()
}

/// The first letter of each word, and the first consonant of each Hangul
/// syllable: "Brave Browser" gives "BB", "카카오톡" "ㅋㅋㅇㅌ". A capital after a
/// small letter starts a word ("KakaoTalk" gives "KT"), and so does a letter
/// after Hangul ("네이버Works" gives "ㄴㅇㅂW").
fn initials(text: &str) -> String {
    let mut out = String::new();
    let mut word_start = true;
    let mut after_lower = false;
    for c in text.chars() {
        if let Some(syllable) = syllable(c) {
            out.push(CHOSEONG[syllable.0]);
            word_start = true;
            after_lower = false;
        } else if c.is_alphanumeric() {
            if word_start || (after_lower && c.is_uppercase()) {
                out.push(c);
            }
            word_start = false;
            after_lower = c.is_lowercase();
        } else {
            word_start = true;
            after_lower = false;
        }
    }
    out
}

fn has_hangul(text: &str) -> bool {
    text.chars().any(|c| syllable(c).is_some() || jamo_keys(c).is_some())
}

/// The keys of a QWERTY keyboard that type `text` with the Korean two-set
/// layout, lowercased; other characters stay. Both layouts' text then compare
/// in one alphabet.
fn keys(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if let Some((initial, medial, last)) = syllable(c) {
            out.push_str(INITIAL_KEYS[initial]);
            out.push_str(MEDIAL_KEYS[medial]);
            out.push_str(FINAL_KEYS[last]);
        } else if let Some(keys) = jamo_keys(c) {
            out.push_str(keys);
        } else {
            out.push(c);
        }
    }
    out.to_lowercase()
}

/// A Hangul syllable's initial, medial and final (0: none) indices.
fn syllable(c: char) -> Option<(usize, usize, usize)> {
    let code = (c as u32).checked_sub(0xAC00).filter(|code| *code < 11172)? as usize;
    Some((code / (21 * 28), code % (21 * 28) / 28, code % 28))
}

const CHOSEONG: [char; 19] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ',
    'ㅌ', 'ㅍ', 'ㅎ',
];
const INITIAL_KEYS: [&str; 19] = [
    "r", "R", "s", "e", "E", "f", "a", "q", "Q", "t", "T", "d", "w", "W", "c", "z", "x", "v", "g",
];
const MEDIAL_KEYS: [&str; 21] = [
    "k", "o", "i", "O", "j", "p", "u", "P", "h", "hk", "ho", "hl", "y", "n", "nj", "np", "nl", "b",
    "m", "ml", "l",
];
const FINAL_KEYS: [&str; 28] = [
    "", "r", "R", "rt", "s", "sw", "sg", "e", "f", "fr", "fa", "fq", "ft", "fx", "fv", "fg", "a",
    "q", "qt", "t", "T", "d", "w", "c", "z", "x", "v", "g",
];
/// The compatibility letters ㄱ (U+3131) to ㅣ (U+3163), as typed alone.
const JAMO_KEYS: [&str; 51] = [
    "r", "R", "rt", "s", "sw", "sg", "e", "E", "f", "fr", "fa", "fq", "ft", "fx", "fv", "fg", "a",
    "q", "Q", "qt", "t", "T", "d", "w", "W", "c", "z", "x", "v", "g", "k", "o", "i", "O", "j", "p",
    "u", "P", "h", "hk", "ho", "hl", "y", "n", "nj", "np", "nl", "b", "m", "ml", "l",
];

fn jamo_keys(c: char) -> Option<&'static str> {
    let index = (c as u32).checked_sub(0x3131)? as usize;
    JAMO_KEYS.get(index).copied()
}

/// Lowercase, with the accents of Latin letters taken off. Hangul and other
/// scripts are kept as they are.
pub fn fold(text: &str) -> String { text.chars().flat_map(char::to_lowercase).map(plain).collect() }

fn plain(c: char) -> char {
    match c {
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' | 'ħ' => 'h',
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' | 'ŧ' => 't',
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, others: &[&str]) -> App {
        App {
            bundle_id: format!("dev.{name}"),
            path: format!("/Applications/{name}.app").into(),
            name: name.into(),
            other_names: others.iter().map(|o| o.to_string()).collect(),
            category_type: None,
            browser: false,
        }
    }

    fn names(apps: &[App], query: &str) -> Vec<String> {
        search(apps, query).into_iter().map(|i| apps[i].name.clone()).collect()
    }

    #[test]
    fn starts_then_holds_then_other_names() {
        let apps = [
            app("Activity Monitor", &[]),
            app("계산기", &["Calculator"]),
            app("Mail", &[]),
            app("Monitor Control", &[]),
            app("System Monitor", &[]),
        ];
        assert_eq!(names(&apps, "mon"), [
            "Monitor Control",
            "Activity Monitor",
            "System Monitor"
        ]);
        assert_eq!(names(&apps, "calc"), ["계산기"]);
        assert_eq!(names(&apps, "계산"), ["계산기"]);
        assert_eq!(names(&apps, "monitor activity"), ["Activity Monitor"]);
        assert_eq!(names(&apps, "  "), Vec::<String>::new());
        assert_eq!(names(&apps, "xyz"), Vec::<String>::new());
    }

    #[test]
    fn words_may_be_split_between_the_name_and_other_names() {
        let apps = [app("터미널", &["Terminal"])];
        assert_eq!(names(&apps, "터미 term"), ["터미널"]);
    }

    #[test]
    fn initials_find_words_and_syllables() {
        let apps = [
            app("Brave Browser", &[]),
            app("Bluetooth File Exchange", &[]),
            app("카카오톡", &["KakaoTalk"]),
            app("Visual Studio Code", &["Code"]),
        ];
        assert_eq!(names(&apps, "bb"), ["Brave Browser"]);
        assert_eq!(names(&apps, "bfe"), ["Bluetooth File Exchange"]);
        assert_eq!(names(&apps, "ㅋㅋㅇㅌ"), ["카카오톡"]);
        assert_eq!(names(&apps, "ㅋㅋ"), ["카카오톡"]);
        assert_eq!(names(&apps, "vsc"), ["Visual Studio Code"]);
        assert_eq!(initials("google chrome"), "gc");
        assert_eq!(initials("밀리의서재"), "ㅁㄹㅇㅅㅈ");
        assert_eq!(initials("KakaoTalk"), "KT");
        assert_eq!(initials("네이버Works"), "ㄴㅇㅂW");
        assert_eq!(initials("ONLYOFFICE"), "O");
    }

    #[test]
    fn a_search_typed_in_the_other_layout_still_finds() {
        let apps = [
            app("Google Chrome", &[]),
            app("카카오톡", &["KakaoTalk"]),
            app("Calculator", &[]),
        ];
        // "chrome" with the Korean layout on; "chro" composes as 초개.
        assert_eq!(names(&apps, "초개ㅡㄷ"), ["Google Chrome"]);
        assert_eq!(names(&apps, "초개"), ["Google Chrome"]);
        // 카카오톡 with it off.
        assert_eq!(names(&apps, "zkzkdhxhr"), ["카카오톡"]);
        assert_eq!(names(&apps, "zkzk"), ["카카오톡"]);
        assert_eq!(keys("괜찮"), "rhoscksg");
        assert_eq!(keys("ㅘ ㅃ"), "hk q");
    }

    #[test]
    fn a_korean_search_does_not_bring_every_latin_name_with_that_key() {
        let apps = [
            app("Brave Browser", &[]),
            app("Terminal", &[]),
            app("계산기", &["Calculator"]),
        ];
        // ㄱ is the r key: not in the middle of "Brave" or "Terminal".
        assert_eq!(names(&apps, "ㄱ"), ["계산기"]);
        assert_eq!(names(&apps, "계"), ["계산기"]);
        // A Latin search is not read against Latin names in key space.
        assert_eq!(names(&apps, "xyz"), Vec::<String>::new());
    }

    #[test]
    fn the_right_layout_comes_before_the_other_one() {
        // "rk" is 가 typed with the Korean layout off, and also in "Park".
        let apps = [app("가계부", &[]), app("Park", &[])];
        assert_eq!(names(&apps, "rk"), ["Park", "가계부"]);
    }

    #[test]
    fn case_and_accents_are_ignored() {
        let apps = [app("Café Écrit", &[])];
        assert_eq!(names(&apps, "CAFE ecr"), ["Café Écrit"]);
        assert_eq!(fold("ÀÉÎõü 한글"), "aeiou 한글");
    }
}

//! Matching the search against the apps, in subline's order (docs/home.md,
//! "Search").

use super::apps::App;

/// The apps that match `query`, best first, as indices into `apps`. Each word
/// of the search must be in the name or one of the other names, case and
/// accents ignored. Names that start with the search come first, then names
/// that hold every word, then apps found only through their other names;
/// within each, the order of `apps` (by name).
pub fn search(apps: &[App], query: &str) -> Vec<usize> {
    let query = fold(query.trim());
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut tiers: [Vec<usize>; 3] = Default::default();
    for (index, app) in apps.iter().enumerate() {
        let name = fold(&app.name);
        let tier = if name.starts_with(&query) {
            0
        } else if words.iter().all(|word| name.contains(word)) {
            1
        } else {
            let others: Vec<String> = app.other_names.iter().map(|other| fold(other)).collect();
            let found =
                |word: &&str| name.contains(word) || others.iter().any(|o| o.contains(word));
            if !words.iter().all(found) {
                continue;
            }
            2
        };
        tiers[tier].push(index);
    }
    tiers.concat()
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
    fn case_and_accents_are_ignored() {
        let apps = [app("Café Écrit", &[])];
        assert_eq!(names(&apps, "CAFE ecr"), ["Café Écrit"]);
        assert_eq!(fold("ÀÉÎõü 한글"), "aeiou 한글");
    }
}

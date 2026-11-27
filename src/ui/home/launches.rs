//! When each app was opened from home, for the frequent row
//! (`~/.rift-grid/launches.json`, docs/home.md "The grid").

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The times kept for each app.
const KEPT: usize = 20;
/// An opening counts half as much after this many days.
const HALF_LIFE_DAYS: f64 = 7.0;
const DAY: f64 = 86_400.0;

/// Seconds since 1970 of each opening, by bundle id (lowercased), oldest first.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Launches(BTreeMap<String, Vec<u64>>);

pub fn path() -> PathBuf { crate::common::config::data_dir().join("launches.json") }

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

impl Launches {
    /// The file's openings; none if it is missing or cannot be read.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Write the file whole, through a file beside it, so a reader never sees
    /// half of it.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let next = path.with_extension("json.new");
        std::fs::write(&next, serde_json::to_vec(self)?)?;
        std::fs::rename(next, path)
    }

    pub fn record(&mut self, bundle_id: &str, at: u64) {
        let times = self.0.entry(bundle_id.to_lowercase()).or_default();
        times.push(at);
        if times.len() > KEPT {
            times.drain(..times.len() - KEPT);
        }
    }

    /// Each opening halved for every 7 days of its age, summed.
    pub fn score(&self, bundle_id: &str, now: u64) -> f64 {
        self.0.get(&bundle_id.to_lowercase()).map_or(0.0, |times| {
            times
                .iter()
                .map(|&at| {
                    let days = now.saturating_sub(at) as f64 / DAY;
                    0.5f64.powf(days / HALF_LIFE_DAYS)
                })
                .sum()
        })
    }

    /// Up to `count` of `ids` (in name order) that were opened, best score
    /// first, ties in the order given.
    pub fn frequent<'a>(
        &self,
        ids: impl IntoIterator<Item = (usize, &'a str)>,
        now: u64,
        count: usize,
    ) -> Vec<usize> {
        let mut scored: Vec<(usize, f64)> = ids
            .into_iter()
            .map(|(index, id)| (index, self.score(id, now)))
            .filter(|(_, score)| *score > 0.0)
            .collect();
        // A stable sort keeps the name order among equal scores.
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.into_iter().take(count).map(|(index, _)| index).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;
    const WEEK: u64 = 7 * 86_400;

    #[test]
    fn an_opening_counts_half_after_a_week() {
        let mut launches = Launches::default();
        launches.record("com.Example.A", NOW - WEEK);
        launches.record("com.example.a", NOW);
        assert!((launches.score("COM.EXAMPLE.A", NOW) - 1.5).abs() < 1e-9);
        assert_eq!(launches.score("com.example.b", NOW), 0.0);
    }

    #[test]
    fn only_the_last_twenty_are_kept() {
        let mut launches = Launches::default();
        for at in 0..25 {
            launches.record("a", NOW - 100 + at);
        }
        assert_eq!(launches.0["a"].len(), 20);
        assert_eq!(launches.0["a"][0], NOW - 95);
    }

    #[test]
    fn frequent_is_by_score_then_by_name_and_skips_the_unopened() {
        let mut launches = Launches::default();
        launches.record("b", NOW);
        launches.record("c", NOW);
        launches.record("d", NOW - 4 * WEEK);
        launches.record("d", NOW - 4 * WEEK);
        launches.record("e", NOW);
        launches.record("e", NOW);
        let ids = [(0, "a"), (1, "b"), (2, "c"), (3, "d"), (4, "e")];
        assert_eq!(launches.frequent(ids, NOW, 3), [4, 1, 2]);
        assert_eq!(launches.frequent(ids, NOW, 9), [4, 1, 2, 3]);
        assert!(Launches::default().frequent(ids, NOW, 3).is_empty());
    }

    #[test]
    fn the_file_round_trips_and_a_bad_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep/launches.json");
        let mut launches = Launches::default();
        launches.record("a", NOW);
        launches.save(&path).unwrap();
        assert_eq!(Launches::load(&path), launches);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Launches::load(&path), Launches::default());
        assert_eq!(
            Launches::load(&dir.path().join("missing.json")),
            Launches::default()
        );
    }
}

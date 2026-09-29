//! Decides what an install or update does to each file, without letting it
//! silently overwrite or delete anything the player changed themselves.
//!
//! Three hashes are compared per file: what Drop last installed (the
//! baseline in `.dropdata`), what is on disk now, and what the new version
//! has. A file the player edited that the new version also changes (or
//! removes) becomes a [`FileConflict`] the player has to answer; files Drop
//! never installed are never touched.

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::UNIX_EPOCH,
};

use database::DownloadableMetadata;
use serde::{Deserialize, Serialize};
use utils::lock;

use super::drop_data::{InstalledFileRecord, hash_file};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    /// The new version has different content for a file the player edited.
    Changed,
    /// The new version no longer has a file the player edited.
    Removed,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileConflict {
    pub path: String,
    pub kind: ConflictKind,
    #[serde(skip)]
    pub target_hash: Option<String>,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    /// Replace (or remove) the player's copy with the new version's.
    Overwrite,
    /// Leave the player's copy exactly as it is.
    Keep,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct FilePlan {
    /// Files to (re)write, with the hash the new version has for them.
    pub write: BTreeMap<String, Option<String>>,
    /// Files Drop installed, the player didn't change, and the new version
    /// no longer has.
    pub delete: Vec<String>,
    /// Records to drop without touching disk: already gone, or removed from
    /// the game but kept by the player (so they're the player's file now).
    pub untrack: Vec<String>,
    /// Files the new version has that stay as they are on disk. Carries the
    /// on-disk hash when it's known, to refresh the record.
    pub unchanged: BTreeMap<String, Option<String>>,
    /// Player-edited files whose new content the player chose not to take.
    pub declined: BTreeMap<String, Option<String>>,
    /// Waiting on the player: see [`FilePlan::resolve`].
    pub conflicts: Vec<FileConflict>,
}

/// Where the plan reads the current state of a file from.
pub trait Disk {
    /// Hash of the file's current content, or `None` when there's no regular
    /// file there. `record` lets an implementation skip hashing a file whose
    /// metadata shows it hasn't changed since Drop wrote it.
    fn hash(&mut self, path: &str, record: Option<&InstalledFileRecord>) -> DiskState;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskState {
    Missing,
    Present(String),
    /// Something Drop won't read through, e.g. a symlink. Treated as the
    /// player's.
    Foreign,
}

pub fn compute_plan(
    records: &HashMap<String, InstalledFileRecord>,
    file_list: &HashMap<String, String>,
    file_hashes: &HashMap<String, String>,
    disk: &mut dyn Disk,
) -> FilePlan {
    let mut plan = FilePlan::default();

    for (path, owner) in file_list {
        let target = file_hashes.get(path).cloned();
        let record = records.get(path);

        // Drop was already replacing this file: whatever is there is ours.
        if record.is_some_and(|r| r.pending_hash.is_some()) {
            plan.write.insert(path.clone(), target);
            continue;
        }

        let current = match disk.hash(path, record) {
            DiskState::Missing => {
                plan.write.insert(path.clone(), target);
                continue;
            }
            DiskState::Present(hash) => Some(hash),
            DiskState::Foreign => None,
        };

        if current.is_some() && current == target {
            plan.unchanged.insert(path.clone(), current);
            continue;
        }

        let baseline = record.and_then(|r| r.baseline());
        let player_changed = match (record, baseline) {
            // Drop never installed this file: it's the player's.
            (None, _) => true,
            (Some(_), Some(baseline)) => current.as_deref() != Some(baseline),
            // Recorded before hashes were tracked: assume it's Drop's.
            (Some(_), None) => current.is_none(),
        };
        let update_changes = match (&target, baseline) {
            (Some(target), Some(baseline)) => target != baseline,
            _ => record.and_then(|r| r.owner.as_deref()) != Some(owner.as_str()),
        };

        if !update_changes {
            plan.unchanged.insert(path.clone(), current);
        } else if !player_changed {
            plan.write.insert(path.clone(), target);
        } else if target.is_some()
            && record.and_then(|r| r.declined_hash.as_ref()) == target.as_ref()
        {
            plan.declined.insert(path.clone(), target);
        } else {
            plan.conflicts.push(FileConflict {
                path: path.clone(),
                kind: ConflictKind::Changed,
                target_hash: target,
            });
        }
    }

    for (path, record) in records {
        if file_list.contains_key(path) {
            continue;
        }
        if record.pending_hash.is_some() {
            plan.delete.push(path.clone());
            continue;
        }
        match disk.hash(path, Some(record)) {
            DiskState::Missing => plan.untrack.push(path.clone()),
            DiskState::Present(hash)
                if record.baseline().is_none_or(|baseline| baseline == hash) =>
            {
                plan.delete.push(path.clone());
            }
            _ => plan.conflicts.push(FileConflict {
                path: path.clone(),
                kind: ConflictKind::Removed,
                target_hash: None,
            }),
        }
    }

    plan.delete.sort();
    plan.untrack.sort();
    plan.conflicts.sort_by(|a, b| a.path.cmp(&b.path));
    plan
}

impl FilePlan {
    /// Applies the player's answers. A conflict without an answer is kept,
    /// so nothing of theirs is lost by default.
    pub fn resolve(&mut self, choices: &HashMap<String, ConflictChoice>) {
        for conflict in std::mem::take(&mut self.conflicts) {
            let choice = choices
                .get(&conflict.path)
                .copied()
                .unwrap_or(ConflictChoice::Keep);
            match (conflict.kind, choice) {
                (ConflictKind::Changed, ConflictChoice::Overwrite) => {
                    self.write.insert(conflict.path, conflict.target_hash);
                }
                (ConflictKind::Changed, ConflictChoice::Keep) => {
                    self.declined.insert(conflict.path, conflict.target_hash);
                }
                (ConflictKind::Removed, ConflictChoice::Overwrite) => {
                    self.delete.push(conflict.path);
                }
                (ConflictKind::Removed, ConflictChoice::Keep) => {
                    self.untrack.push(conflict.path);
                }
            }
        }
    }
}

/// Reads files under an install directory, skipping the hash for files whose
/// size and modified time still match what Drop recorded after writing them.
pub struct InstallDisk {
    pub base_path: PathBuf,
}

pub fn file_metadata(path: &Path) -> Option<(u64, u64)> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((metadata.len(), u64::try_from(modified).ok()?))
}

impl Disk for InstallDisk {
    fn hash(&mut self, path: &str, record: Option<&InstalledFileRecord>) -> DiskState {
        let full = self.base_path.join(path);
        // Never follow symlinks: they could point anywhere, and Drop doesn't
        // create them.
        let Ok(metadata) = fs::symlink_metadata(&full) else {
            return DiskState::Missing;
        };
        if metadata.file_type().is_symlink() {
            return DiskState::Foreign;
        }
        if !metadata.is_file() {
            return DiskState::Foreign;
        }

        if let Some(record) = record
            && let (Some(size), Some(modified), Some(hash)) =
                (record.size, record.modified_ns, record.client_hash.as_ref())
            && file_metadata(&full) == Some((size, modified))
        {
            return DiskState::Present(hash.clone());
        }

        match hash_file(&full) {
            Ok(hash) => DiskState::Present(hash),
            Err(_) => DiskState::Foreign,
        }
    }
}

/// What the frontend shows the player while a download waits on them.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PendingConflicts {
    pub meta: DownloadableMetadata,
    pub game_name: String,
    pub conflicts: Vec<FileConflict>,
}

struct PendingEntry {
    info: PendingConflicts,
    answer: Option<HashMap<String, ConflictChoice>>,
}

// Keyed by game ID: only one download per game runs at a time.
static PENDING: LazyLock<Mutex<HashMap<String, PendingEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn register_pending(info: PendingConflicts) {
    lock!(PENDING).insert(info.meta.id.clone(), PendingEntry { info, answer: None });
}

/// Takes the player's answer, if they've given one yet.
pub fn take_answer(game_id: &str) -> Option<HashMap<String, ConflictChoice>> {
    let mut pending = lock!(PENDING);
    let answer = pending.get_mut(game_id)?.answer.take()?;
    pending.remove(game_id);
    Some(answer)
}

pub fn clear_pending(game_id: &str) {
    lock!(PENDING).remove(game_id);
}

/// Records the player's answer. Returns false when no download is waiting.
pub fn answer_pending(game_id: &str, choices: HashMap<String, ConflictChoice>) -> bool {
    match lock!(PENDING).get_mut(game_id) {
        Some(entry) => {
            entry.answer = Some(choices);
            true
        }
        None => false,
    }
}

pub fn list_pending() -> Vec<PendingConflicts> {
    lock!(PENDING)
        .values()
        .filter(|entry| entry.answer.is_none())
        .map(|entry| entry.info.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeDisk(HashMap<String, DiskState>);
    impl Disk for FakeDisk {
        fn hash(&mut self, path: &str, _: Option<&InstalledFileRecord>) -> DiskState {
            self.0.get(path).cloned().unwrap_or(DiskState::Missing)
        }
    }

    fn rec(hash: &str, owner: &str) -> InstalledFileRecord {
        InstalledFileRecord {
            server_hash: Some(hash.into()),
            owner: Some(owner.into()),
            ..Default::default()
        }
    }
    fn map<V: Clone>(entries: &[(&str, V)]) -> HashMap<String, V> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }
    fn present(hash: &str) -> DiskState {
        DiskState::Present(hash.into())
    }
    fn plan(
        records: &[(&str, InstalledFileRecord)],
        list: &[(&str, &str)],
        hashes: &[(&str, &str)],
        disk: &[(&str, DiskState)],
    ) -> FilePlan {
        let list: HashMap<String, String> = list
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let hashes: HashMap<String, String> = hashes
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        compute_plan(&map(records), &list, &hashes, &mut FakeDisk(map(disk)))
    }

    #[test]
    fn untouched_file_changed_upstream_is_rewritten_even_with_same_owner() {
        // The resync case: same owning version, new content.
        let p = plan(
            &[("level.pak", rec("old", "v1"))],
            &[("level.pak", "v1")],
            &[("level.pak", "new")],
            &[("level.pak", present("old"))],
        );
        assert_eq!(p.write.keys().collect::<Vec<_>>(), ["level.pak"]);
        assert!(p.conflicts.is_empty());
    }

    #[test]
    fn unchanged_file_is_skipped() {
        let p = plan(
            &[("a", rec("h", "v1"))],
            &[("a", "v2")],
            &[("a", "h")],
            &[("a", present("h"))],
        );
        assert!(p.write.is_empty());
        assert!(p.unchanged.contains_key("a"));
    }

    #[test]
    fn player_edit_the_update_doesnt_touch_is_left_alone() {
        let p = plan(
            &[("config.ini", rec("h", "v1"))],
            &[("config.ini", "v1")],
            &[("config.ini", "h")],
            &[("config.ini", present("edited"))],
        );
        assert!(p.write.is_empty() && p.conflicts.is_empty());
        assert!(p.unchanged.contains_key("config.ini"));
    }

    #[test]
    fn player_edit_the_update_changes_is_a_conflict() {
        let p = plan(
            &[("config.ini", rec("h", "v1"))],
            &[("config.ini", "v2")],
            &[("config.ini", "h2")],
            &[("config.ini", present("edited"))],
        );
        assert!(p.write.is_empty());
        assert_eq!(p.conflicts.len(), 1);
        assert_eq!(p.conflicts[0].kind, ConflictKind::Changed);
    }

    #[test]
    fn declined_content_isnt_asked_about_again() {
        let mut r = rec("h", "v1");
        r.declined_hash = Some("h2".into());
        let p = plan(
            &[("config.ini", r)],
            &[("config.ini", "v2")],
            &[("config.ini", "h2")],
            &[("config.ini", present("edited"))],
        );
        assert!(p.conflicts.is_empty());
        assert!(p.declined.contains_key("config.ini"));
    }

    #[test]
    fn removed_files_are_deleted_only_if_unedited_and_untracked_files_are_ignored() {
        let p = plan(
            &[("gone.pak", rec("g", "v1")), ("edited.pak", rec("e", "v1"))],
            &[],
            &[],
            &[
                ("gone.pak", present("g")),
                ("edited.pak", present("mine")),
                ("mods/mod.pak", present("m")),
            ],
        );
        assert_eq!(p.delete, ["gone.pak"]);
        assert_eq!(p.conflicts.len(), 1);
        assert_eq!(p.conflicts[0].path, "edited.pak");
        assert_eq!(p.conflicts[0].kind, ConflictKind::Removed);
    }

    #[test]
    fn half_written_file_from_a_paused_update_is_drops() {
        let mut r = rec("old", "v1");
        r.pending_hash = Some("new".into());
        let p = plan(
            &[("big.pak", r)],
            &[("big.pak", "v2")],
            &[("big.pak", "new")],
            &[("big.pak", present("half"))],
        );
        assert!(p.write.contains_key("big.pak"));
        assert!(p.conflicts.is_empty());
    }

    #[test]
    fn a_players_own_file_where_the_update_adds_one_is_a_conflict() {
        let p = plan(
            &[],
            &[("new.txt", "v2")],
            &[("new.txt", "n")],
            &[("new.txt", present("players"))],
        );
        assert_eq!(p.conflicts.len(), 1);
        assert!(p.write.is_empty());
    }

    #[test]
    fn symlinks_are_the_players() {
        let p = plan(
            &[("a", rec("h", "v1"))],
            &[("a", "v2")],
            &[("a", "h2")],
            &[("a", DiskState::Foreign)],
        );
        assert_eq!(p.conflicts.len(), 1);
    }

    #[test]
    fn resolve_applies_choices_and_keeps_unanswered() {
        let mut p = plan(
            &[
                ("a", rec("h", "v1")),
                ("b", rec("h", "v1")),
                ("c", rec("h", "v1")),
            ],
            &[("a", "v2"), ("b", "v2")],
            &[("a", "h2"), ("b", "h2")],
            &[
                ("a", present("x")),
                ("b", present("x")),
                ("c", present("x")),
            ],
        );
        assert_eq!(p.conflicts.len(), 3);
        p.resolve(&map(&[
            ("a", ConflictChoice::Overwrite),
            ("c", ConflictChoice::Overwrite),
        ]));
        assert!(p.write.contains_key("a"));
        assert!(p.declined.contains_key("b"));
        assert_eq!(p.delete, ["c"]);
        assert!(p.conflicts.is_empty());
    }

    /// A scratch install directory that's removed again when dropped.
    struct TempInstall(PathBuf);
    impl TempInstall {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("drop-file-plan-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn write(&self, path: &str, content: &[u8]) {
            let full = self.0.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
        fn disk(&self) -> InstallDisk {
            InstallDisk {
                base_path: self.0.clone(),
            }
        }
    }
    impl Drop for TempInstall {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn install_disk_hashes_files_and_reports_missing_ones() {
        let install = TempInstall::new("hash");
        install.write("bin/game.exe", b"game");
        let mut disk = install.disk();

        let expected = hash_file(&install.0.join("bin/game.exe")).unwrap();
        assert_eq!(
            disk.hash("bin/game.exe", None),
            DiskState::Present(expected)
        );
        assert_eq!(disk.hash("missing.pak", None), DiskState::Missing);
    }

    #[test]
    fn install_disk_trusts_the_recorded_hash_while_metadata_matches() {
        let install = TempInstall::new("shortcut");
        install.write("data.pak", b"content");
        let (size, modified) = file_metadata(&install.0.join("data.pak")).unwrap();
        let record = InstalledFileRecord {
            client_hash: Some("recorded".into()),
            size: Some(size),
            modified_ns: Some(modified),
            ..Default::default()
        };
        let mut disk = install.disk();

        // Unchanged metadata: the recorded hash is used without reading.
        assert_eq!(
            disk.hash("data.pak", Some(&record)),
            DiskState::Present("recorded".into())
        );

        // Edited (the size changed): the file is hashed for real.
        install.write("data.pak", b"edited by the player");
        let actual = hash_file(&install.0.join("data.pak")).unwrap();
        assert_eq!(
            disk.hash("data.pak", Some(&record)),
            DiskState::Present(actual)
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_disk_never_follows_symlinks() {
        let install = TempInstall::new("symlink");
        install.write("real.txt", b"target");
        std::os::unix::fs::symlink(install.0.join("real.txt"), install.0.join("link.txt")).unwrap();
        assert_eq!(install.disk().hash("link.txt", None), DiskState::Foreign);
    }

    #[test]
    fn plan_against_a_real_install() {
        let install = TempInstall::new("plan");
        install.write("untouched.pak", b"v1");
        install.write("edited.ini", b"player's settings");
        install.write("mods/mine.pak", b"a mod");
        let v1 = hash_file(&install.0.join("untouched.pak")).unwrap();
        let records = map(&[
            ("untouched.pak", rec(&v1, "v1")),
            ("edited.ini", rec("original-ini", "v1")),
        ]);
        let list = map(&[
            ("untouched.pak", "v2".to_string()),
            ("edited.ini", "v2".to_string()),
        ]);
        let hashes = map(&[
            ("untouched.pak", "v2-content".to_string()),
            ("edited.ini", "v2-ini".to_string()),
        ]);

        let p = compute_plan(&records, &list, &hashes, &mut install.disk());
        assert_eq!(p.write.keys().collect::<Vec<_>>(), ["untouched.pak"]);
        assert_eq!(p.conflicts.len(), 1);
        assert_eq!(p.conflicts[0].path, "edited.ini");
        // The mod was never Drop's, so the plan doesn't mention it at all.
        assert!(p.delete.is_empty() && p.untrack.is_empty());
    }

    #[test]
    fn a_tracked_file_thats_already_gone_is_just_untracked() {
        let p = plan(&[("old.pak", rec("h", "v1"))], &[], &[], &[]);
        assert_eq!(p.untrack, ["old.pak"]);
        assert!(p.delete.is_empty() && p.conflicts.is_empty());
    }

    #[test]
    fn pending_answers_round_trip_through_the_registry() {
        use database::{DownloadType, DownloadableMetadata, platform::Platform};

        let game_id = "registry-test-game";
        register_pending(PendingConflicts {
            meta: DownloadableMetadata::new(
                game_id.into(),
                "v2".into(),
                Platform::Linux,
                DownloadType::Game,
            ),
            game_name: "Test".into(),
            conflicts: vec![FileConflict {
                path: "a".into(),
                kind: ConflictKind::Changed,
                target_hash: None,
            }],
        });
        assert!(list_pending().iter().any(|p| p.meta.id == game_id));
        assert_eq!(take_answer(game_id), None);

        assert!(answer_pending(
            game_id,
            map(&[("a", ConflictChoice::Overwrite)])
        ));
        // Answered downloads stop being offered to a newly opened window.
        assert!(!list_pending().iter().any(|p| p.meta.id == game_id));
        assert_eq!(
            take_answer(game_id),
            Some(map(&[("a", ConflictChoice::Overwrite)]))
        );
        // Taking the answer clears it; answering again finds nothing waiting.
        assert!(!answer_pending(game_id, HashMap::new()));
    }

    #[test]
    fn a_paused_prompt_is_cleared() {
        use database::{DownloadType, DownloadableMetadata, platform::Platform};

        let game_id = "registry-clear-game";
        register_pending(PendingConflicts {
            meta: DownloadableMetadata::new(
                game_id.into(),
                "v2".into(),
                Platform::Linux,
                DownloadType::Game,
            ),
            game_name: "Test".into(),
            conflicts: vec![],
        });
        clear_pending(game_id);
        assert!(!list_pending().iter().any(|p| p.meta.id == game_id));
        assert!(!answer_pending(game_id, HashMap::new()));
    }
}

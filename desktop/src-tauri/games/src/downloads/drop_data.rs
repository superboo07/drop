use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use database::{models::data::UserConfiguration, platform::Platform};
use log::error;
use sha2::{Digest, Sha256};
use utils::lock;

pub type DropData = v1::DropData;
pub type InstalledFileRecord = v1::InstalledFileRecord;

/// Whole-file SHA-256 (hex) of a file's content, streamed rather than read
/// fully into memory. Shared between download completion (to compute
/// `client_hash`/verify against `server_hash`) and uninstall (to check a
/// currently-installed file against either).
pub fn hash_file(path: &Path) -> Result<String, io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let amount = file.read(&mut buf)?;
        if amount == 0 {
            break;
        }
        hasher.update(&buf[..amount]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub static DROPDATA_PATH: &str = ".dropdata";

pub mod v1 {
    use std::{collections::HashMap, path::PathBuf, sync::Mutex};

    use database::{models::data::UserConfiguration, platform::Platform};
    use serde::{Deserialize, Serialize};

    // Content-verification info for a single installed file, keyed by path
    // relative to base_path elsewhere. `server_hash` comes from the server's
    // manifest (authoritative, when available); `client_hash` is computed
    // locally right after a completed download as a fallback for when no
    // server hash exists (offline at uninstall time is a non-issue either
    // way, since both are cached locally at install/update time already).
    //
    // The rest is what lets an update tell the player's own edits apart from
    // what Drop put there (see `file_plan`):
    // - `owner`: the server version that supplied this content, for spotting
    //   changes when the server has no hash for a file.
    // - `size`/`modified_ns`: the file's metadata right after Drop wrote and
    //   verified it. While both still match, the file is known to be
    //   untouched without re-hashing it.
    // - `pending_hash`: Drop has started replacing this file with content of
    //   this hash, so whatever is on disk now is Drop's half-written file, not
    //   an edit by the player.
    // - `declined_hash`: the player chose to keep their own copy instead of
    //   this content; they aren't asked about the same content again.
    #[derive(Serialize, Deserialize, Clone, Debug, Default)]
    pub struct InstalledFileRecord {
        #[serde(default)]
        pub server_hash: Option<String>,
        #[serde(default)]
        pub client_hash: Option<String>,
        #[serde(default)]
        pub owner: Option<String>,
        #[serde(default)]
        pub size: Option<u64>,
        #[serde(default)]
        pub modified_ns: Option<u64>,
        #[serde(default)]
        pub pending_hash: Option<String>,
        #[serde(default)]
        pub declined_hash: Option<String>,
    }

    impl InstalledFileRecord {
        /// Hash of the content Drop last installed at this path, if known.
        pub fn baseline(&self) -> Option<&str> {
            self.server_hash.as_deref().or(self.client_hash.as_deref())
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct DropData {
        pub game_id: String,
        pub game_version: String,
        pub target_platform: Platform,
        #[serde(default)]
        pub configuration: UserConfiguration,
        pub contexts: Mutex<HashMap<String, bool>>,
        pub base_path: PathBuf,
        pub previously_installed_version: Option<String>,
        // Every file the install/update process wrote, as reported by the
        // server manifest, keyed by path relative to base_path. Lets uninstall
        // remove only what Drop actually created (and only if its content is
        // still what was installed) instead of the whole install directory.
        // Missing on .dropdata files written before this field existed - falls
        // back to an empty map, which callers must treat as "unknown", not
        // "nothing installed".
        #[serde(default)]
        pub installed_files: Mutex<HashMap<String, InstalledFileRecord>>,
    }

    impl DropData {
        pub fn new(
            game_id: String,
            game_version: String,
            target_platform: Platform,
            base_path: PathBuf,
            configuration: UserConfiguration,
            previously_installed_version: Option<String>,
        ) -> Self {
            Self {
                base_path,
                game_id,
                game_version,
                target_platform,
                contexts: Mutex::new(HashMap::new()),
                configuration,
                previously_installed_version,
                installed_files: Mutex::new(HashMap::new()),
            }
        }
    }
}

impl DropData {
    pub fn generate(
        game_id: String,
        game_version: String,
        target_platform: Platform,
        base_path: PathBuf,
        configuration: UserConfiguration,
    ) -> Self {
        match DropData::read(&base_path) {
            Ok(v) => {
                if v.game_id != game_id || v.game_version != game_version {
                    let previous_files = v.get_installed_files();
                    let new = DropData::new(
                        game_id,
                        game_version,
                        target_platform,
                        base_path,
                        configuration,
                        Some(v.game_version),
                    );
                    // What the previous version installed is the baseline the
                    // update compares against to find the player's own edits.
                    if v.game_id == new.game_id {
                        new.set_installed_files(previous_files);
                    }
                    return new;
                }
                v
            }
            Err(_) => DropData::new(
                game_id,
                game_version,
                target_platform,
                base_path,
                configuration,
                None,
            ),
        }
    }
    pub fn read(base_path: &Path) -> Result<Self, io::Error> {
        let mut file = File::open(base_path.join(DROPDATA_PATH))?;

        let mut s = Vec::new();
        file.read_to_end(&mut s)?;

        pot::from_slice(&s).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to decode drop data: {e}"),
            )
        })
    }
    pub fn write(&self) {
        let manifest_raw = match pot::to_vec(&self) {
            Ok(data) => data,
            Err(_) => return,
        };

        let mut file = match File::create(self.base_path.join(DROPDATA_PATH)) {
            Ok(file) => file,
            Err(e) => {
                error!("{e}");
                return;
            }
        };

        match file.write_all(&manifest_raw) {
            Ok(()) => {}
            Err(e) => error!("{e}"),
        }
    }
    pub fn set_contexts(&self, completed_contexts: &[(String, bool)]) {
        *lock!(self.contexts) = completed_contexts
            .iter()
            .map(|s| (s.0.clone(), s.1))
            .collect();
    }
    pub fn set_context(&self, context: String, state: bool) {
        lock!(self.contexts).entry(context).insert_entry(state);
    }
    pub fn get_contexts(&self) -> HashMap<String, bool> {
        lock!(self.contexts).clone()
    }
    pub fn set_installed_files(&self, installed_files: HashMap<String, InstalledFileRecord>) {
        *lock!(self.installed_files) = installed_files;
    }
    pub fn get_installed_files(&self) -> HashMap<String, InstalledFileRecord> {
        lock!(self.installed_files).clone()
    }
    pub fn set_installed_file_client_hash(&self, path: &str, hash: String) {
        lock!(self.installed_files)
            .entry(path.to_string())
            .or_default()
            .client_hash = Some(hash);
    }
}

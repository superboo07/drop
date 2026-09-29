pub mod data {
    use std::{hash::Hash, path::PathBuf};

    use serde::{Deserialize, Serialize};

    // NOTE: Within each version, you should NEVER use these types.
    // Declare it using the actual version that it is from, i.e. v1::Settings rather than just Settings from here

    pub type Database = v1::Database;
    pub type GameVersion = v1::GameVersion;
    pub type Settings = v1::Settings;
    pub type UpdateBranch = v1::UpdateBranch;
    pub type EmulatorOverride = v1::EmulatorOverride;
    pub type EmulatorOverrideKind = v1::EmulatorOverrideKind;
    pub type LocalEmulator = v1::LocalEmulator;
    pub type DatabaseAuth = v1::DatabaseAuth;

    pub type GameDownloadStatus = v1::GameDownloadStatus;
    pub type InstalledGameType = v1::InstalledGameType;
    pub type ApplicationTransientStatus = v1::ApplicationTransientStatus;
    /**
     * Need to be universally accessible by the ID, and the version is just a couple sprinkles on top
     */
    pub type DownloadableMetadata = v1::DownloadableMetadata;
    pub type DownloadType = v1::DownloadType;
    pub type DatabaseApplications = v1::DatabaseApplications;
    pub type UserConfiguration = v1::UserConfiguration;
    pub type PendingPlaytimeSession = v1::PendingPlaytimeSession;

    use std::collections::HashMap;

    impl PartialEq for DownloadableMetadata {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id && self.download_type == other.download_type
        }
    }
    impl Hash for DownloadableMetadata {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.id.hash(state);
            self.download_type.hash(state);
        }
    }

    #[derive(Serialize, Deserialize)]
    enum DatabaseVersionEnum {
        V0_4_0 { database: v1::Database },
    }

    pub struct DatabaseVersionSerializable(pub(crate) Database);

    impl Serialize for DatabaseVersionSerializable {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            // Always serialize to latest version
            DatabaseVersionEnum::V0_4_0 {
                database: self.0.clone(),
            }
            .serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for DatabaseVersionSerializable {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            Ok(match DatabaseVersionEnum::deserialize(deserializer)? {
                DatabaseVersionEnum::V0_4_0 { database } => DatabaseVersionSerializable(database),
            })
        }
    }

    mod v1 {
        use serde_with::serde_as;
        use std::{collections::HashMap, path::PathBuf};

        use crate::platform::Platform;

        use super::{Deserialize, Serialize};

        fn default_template() -> UserConfiguration {
            UserConfiguration {
                launch_template: "{}".to_owned(),
                override_proton_path: None,
                override_handler: None,
                enable_updates: false,
                disable_dxvk: false,
                disable_esync: false,
                disable_fsync: false,
                extra_env_vars: String::new(),
                luna_translator: false,
                nested_session: false,
                locale: None,
            }
        }

        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
        #[serde(rename_all = "camelCase")]
        pub struct UserConfiguration {
            pub launch_template: String,
            pub override_proton_path: Option<String>,
            #[serde(default)]
            pub override_handler: Option<String>,
            pub enable_updates: bool,
            #[serde(default)]
            pub disable_dxvk: bool,
            #[serde(default)]
            pub disable_esync: bool,
            #[serde(default)]
            pub disable_fsync: bool,
            #[serde(default)]
            pub extra_env_vars: String,
            // Launch this game with LunaTranslator attached: the game command
            // gets wrapped in LunaCompanion.exe (which runs inside the same
            // Wine/Proton prefix and streams hooked text back out over TCP),
            // and LunaTranslator itself is spawned alongside it. Linux only -
            // the bridge exists purely to get text out of Wine.
            #[serde(default)]
            pub luna_translator: bool,
            // Run the game and LunaTranslator inside a nested X11 server of
            // our own, so both are ordinary draggable windows instead of
            // fighting over the single surface gamescope will show (Steam
            // Deck game mode). See the `nested_session` crate.
            #[serde(default)]
            pub nested_session: bool,
            // Locale to run this game under (e.g. "ja_JP.UTF-8"), exported
            // as LANG/LC_ALL at launch - Wine derives the Windows system
            // locale and ANSI codepage from these, which is what Japanese
            // visual novels need to not render mojibake. None keeps the
            // host's own locale.
            #[serde(default)]
            pub locale: Option<String>,
        }

        impl Default for UserConfiguration {
            fn default() -> Self {
                default_template()
            }
        }

        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
        #[serde(rename_all = "camelCase")]
        pub struct GameVersion {
            pub game_id: String,
            pub version_id: String,

            pub display_name: Option<String>,
            pub version_path: String,

            pub only_setup: bool,

            pub version_index: usize,
            pub delta: bool,

            #[serde(default = "default_template")]
            pub user_configuration: UserConfiguration,

            pub launches: Vec<LaunchConfiguration>,
            pub setups: Vec<SetupConfiguration>,
        }

        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
        #[serde(rename_all = "camelCase")]
        pub struct LaunchConfiguration {
            pub launch_id: String,

            pub name: String,
            pub command: String,
            pub platform: Platform,
            pub umu_id_override: Option<String>,

            pub emulator: Option<LaunchConfigurationEmulator>,
        }

        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
        #[serde(rename_all = "camelCase")]
        /**
         * This is intended to be used to look up the actual launch configuration that we store elsewhere
         */
        pub struct LaunchConfigurationEmulator {
            pub launch_id: String,
            pub game_id: String,
            pub version_id: String,
        }

        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
        #[serde(rename_all = "camelCase")]
        pub struct SetupConfiguration {
            pub command: String,
            pub platform: Platform,
        }

        #[derive(Serialize, Deserialize, Clone, Debug)]
        #[serde(rename_all = "camelCase")]
        pub struct Settings {
            pub autostart: bool,
            pub max_download_threads: usize,
            pub force_offline: bool,
            #[serde(default)]
            pub quit_on_close: bool,
            #[serde(default)]
            pub windowed_launch_picker: bool,
            #[serde(default)]
            pub start_fullscreen: bool,
            // Fraction of the default 16px root font-size (1.0 = 100%, matching
            // every rem-based Tailwind utility app-wide - see main.scss). A
            // user-controlled slider instead of trying to auto-detect a "correct"
            // scale from window/viewport size: that approach can't see the
            // user's actual screen, so it's a blind guess-and-redeploy loop, and
            // it fights with fixed-size layout (e.g. the custom titlebar) in ways
            // that are hard to predict from window dimensions alone.
            #[serde(default = "default_ui_scale")]
            pub ui_scale: f64,
            // Path to the LunaTranslator AppImage (or any executable that
            // starts it). `None` means the integration is unconfigured and
            // every per-game LunaTranslator toggle is inert.
            #[serde(default)]
            pub luna_translator_path: Option<String>,
            // Override for LunaCompanion.exe, the in-prefix bridge. Normally
            // left unset: it ships inside the LunaTranslator AppImage and we
            // extract our own copy from there (see process::luna).
            #[serde(default)]
            pub luna_bridge_path: Option<String>,
            // Port LunaTranslator listens on for the bridge to connect back
            // to. Matches LunaCompanionServer.LISTEN_PORT upstream.
            #[serde(default = "default_luna_port")]
            pub luna_port: u16,
            // Bring-your-own-emulator mode: when on, a game whose launch
            // option runs through a server-provided emulator uses the user's
            // own local install of that emulator instead (if one is set up
            // in `emulator_overrides`), rather than needing Drop to download
            // the emulator from the server first.
            #[serde(default)]
            pub byo_emulator: bool,
            // Keyed by the emulator's game ID on the server - that's what a
            // launch option's `emulator.game_id` points at.
            #[serde(default)]
            pub emulator_overrides: HashMap<String, EmulatorOverride>, // ... other settings ...
            // Which of the server's client builds this computer is offered:
            // "release" only gets release builds, "test" gets the newest
            // build from either branch.
            #[serde(default)]
            pub update_branch: UpdateBranch,
            // Whether Drop tells you about an update found at startup.
            // Required updates are shown either way.
            #[serde(default = "default_true")]
            pub check_updates_on_start: bool,
        }

        #[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
        #[serde(rename_all = "lowercase")]
        pub enum UpdateBranch {
            #[default]
            Release,
            Test,
        }

        impl UpdateBranch {
            pub fn as_str(&self) -> &'static str {
                match self {
                    UpdateBranch::Release => "release",
                    UpdateBranch::Test => "test",
                }
            }
        }

        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
        #[serde(rename_all = "camelCase")]
        pub struct EmulatorOverride {
            // The emulator's name on the server, so the settings page can
            // still label this entry when the emulator isn't in the library.
            pub name: String,
            // Used for every version of the emulator without its own entry
            // in `versions`. `None` leaves those on the server's copy.
            #[serde(default)]
            pub default: Option<LocalEmulator>,
            // Keyed by the emulator's version ID on the server, for games
            // that need a specific version of it (a launch option's
            // `emulator.version_id`).
            #[serde(default)]
            pub versions: HashMap<String, VersionLocalEmulator>,
            // The install directory that installing a game which runs
            // through this emulator starts on. Only a starting choice - the
            // install dialog can still pick another - and it applies whether
            // or not bring-your-own-emulator mode is on. Ignored if it's no
            // longer one of `install_dirs`.
            #[serde(default)]
            pub install_dir: Option<String>,
        }

        impl EmulatorOverride {
            pub fn for_version(&self, version_id: &str) -> Option<&LocalEmulator> {
                self.versions
                    .get(version_id)
                    .map(|v| &v.emulator)
                    .or(self.default.as_ref())
            }
        }

        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
        #[serde(rename_all = "camelCase")]
        pub struct LocalEmulator {
            #[serde(default)]
            pub kind: EmulatorOverrideKind,
            // The local executable to run, or for a Flatpak, its app ID
            // (e.g. `org.libretro.RetroArch`).
            pub path: String,
            // For a Flatpak, which installed branch to run (e.g. `stable`).
            // None runs whichever branch Flatpak has marked current.
            #[serde(default)]
            pub branch: Option<String>,
            // Arguments passed to it, split shell-style. `{rom}` is replaced
            // with the absolute path of the game's launch target.
            pub args: String,
        }

        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
        #[serde(rename_all = "camelCase")]
        pub struct VersionLocalEmulator {
            // The version's name on the server, for labelling it offline.
            pub version_name: String,
            #[serde(flatten)]
            pub emulator: LocalEmulator,
        }

        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
        #[serde(rename_all = "camelCase")]
        pub enum EmulatorOverrideKind {
            #[default]
            Executable,
            Flatpak,
        }
        fn default_luna_port() -> u16 {
            52300
        }
        fn default_ui_scale() -> f64 {
            1.0
        }
        fn default_true() -> bool {
            true
        }
        impl Default for Settings {
            fn default() -> Self {
                Self {
                    autostart: false,
                    max_download_threads: 4,
                    force_offline: false,
                    quit_on_close: false,
                    windowed_launch_picker: false,
                    start_fullscreen: false,
                    ui_scale: default_ui_scale(),
                    luna_translator_path: None,
                    luna_bridge_path: None,
                    luna_port: default_luna_port(),
                    byo_emulator: false,
                    emulator_overrides: HashMap::new(),
                    update_branch: UpdateBranch::default(),
                    check_updates_on_start: true,
                }
            }
        }

        #[derive(Serialize, Clone, Deserialize, Debug)]
        #[serde(tag = "type")]
        pub enum InstalledGameType {
            SetupRequired,
            Installed,
            PartiallyInstalled {
                #[serde(skip)]
                configuration: UserConfiguration,
            },
        }

        #[derive(Serialize, Clone, Deserialize, Debug)]
        #[serde(tag = "type")]
        pub enum GameDownloadStatus {
            Remote {},
            Installed {
                install_type: InstalledGameType,
                version_id: String,
                install_dir: String,
                update_available: bool,
            },
        }
        // Stuff that shouldn't be synced to disk
        #[derive(Clone, Serialize, Deserialize, Debug)]
        #[serde(tag = "type")]
        pub enum ApplicationTransientStatus {
            Queued { version_id: String },
            Downloading { version_id: String },
            Uninstalling {},
            Updating { version_id: String },
            Validating { version_id: String },
            Running {},
        }

        #[derive(serde::Serialize, Clone, Deserialize)]
        pub struct DatabaseAuth {
            pub private: String,
            pub cert: String,
            pub client_id: String,
            pub web_token: Option<String>,
        }

        #[derive(
            Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Clone, Copy,
        )]
        pub enum DownloadType {
            Game,
            Tool,
            Dlc,
            Mod,
        }

        #[derive(Debug, Eq, PartialOrd, Ord, Serialize, Deserialize, Clone)]
        #[serde(rename_all = "camelCase")]
        pub struct DownloadableMetadata {
            pub id: String,
            pub version: String,
            pub target_platform: Platform,
            pub download_type: DownloadType,
        }
        impl DownloadableMetadata {
            pub fn new(
                id: String,
                version: String,
                target_platform: Platform,
                download_type: DownloadType,
            ) -> Self {
                Self {
                    id,
                    version,
                    target_platform,
                    download_type,
                }
            }
        }

        // A play session not yet acknowledged by the server. Queued here so
        // offline play survives app restarts; drained by the playtime sync
        // scheduler task once online, and only removed once the server
        // explicitly confirms it (see PlaytimeSyncer) - never removed just
        // because a sync request was sent, so a lost response just means a
        // harmless, idempotent resend next cycle rather than lost playtime.
        #[derive(Serialize, Deserialize, Clone, Debug)]
        #[serde(rename_all = "camelCase")]
        pub struct PendingPlaytimeSession {
            pub id: String,
            pub game_id: String,
            pub seconds: u64,
            pub started_at: chrono::DateTime<chrono::Utc>,
            pub ended_at: chrono::DateTime<chrono::Utc>,
        }

        #[serde_as]
        #[derive(Serialize, Clone, Deserialize, Default)]
        #[serde(rename_all = "camelCase")]
        pub struct DatabaseApplications {
            pub install_dirs: Vec<PathBuf>,
            // Guaranteed to exist if the game also exists in the app state map
            pub game_statuses: HashMap<String, GameDownloadStatus>,

            pub game_versions: HashMap<String, GameVersion>,
            pub installed_game_version: HashMap<String, DownloadableMetadata>,

            pub additional_proton_paths: Vec<String>,
            pub default_proton_path: Option<String>,

            #[serde(default)]
            pub pending_playtime_sessions: Vec<PendingPlaytimeSession>,

            #[serde(skip)]
            pub transient_statuses: HashMap<DownloadableMetadata, ApplicationTransientStatus>,
        }

        #[derive(Serialize, Deserialize, Clone, Default)]
        pub struct Database {
            #[serde(default)]
            pub settings: Settings,
            pub auth: Option<DatabaseAuth>,
            pub base_url: String,
            pub applications: DatabaseApplications,
            pub cache_dir: PathBuf,

            #[serde(skip)]
            pub prev_database: Option<PathBuf>,
        }
    }

    impl Database {
        pub fn new<T: Into<PathBuf>>(
            games_base_dir: T,
            prev_database: Option<PathBuf>,
            cache_dir: PathBuf,
        ) -> Self {
            Self {
                applications: DatabaseApplications {
                    install_dirs: vec![games_base_dir.into()],
                    game_statuses: HashMap::new(),
                    game_versions: HashMap::new(),
                    installed_game_version: HashMap::new(),
                    transient_statuses: HashMap::new(),
                    additional_proton_paths: Vec::new(),
                    default_proton_path: None,
                    pending_playtime_sessions: Vec::new(),
                },
                prev_database,
                base_url: String::new(),
                auth: None,
                settings: Settings::default(),
                cache_dir,
            }
        }
    }
    impl DatabaseAuth {
        pub fn new(
            private: String,
            cert: String,
            client_id: String,
            web_token: Option<String>,
        ) -> Self {
            Self {
                private,
                cert,
                client_id,
                web_token,
            }
        }
    }
}

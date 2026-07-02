//! Adds installed games to Steam as "non-Steam game" shortcuts, the same
//! mechanism Lutris uses (see lutris/util/steam/shortcut.py upstream).
//! Steam's shortcuts.vdf is Valve's binary VDF format, not the plaintext
//! KeyValues format -- this implements just enough of it to round-trip an
//! existing file (preserving entries/fields we don't otherwise touch)
//! while adding one new entry.

use std::{fmt, fs, io, path::PathBuf};

use log::info;

const BIN_NONE: u8 = 0x00;
const BIN_STRING: u8 = 0x01;
const BIN_INT32: u8 = 0x02;
const BIN_FLOAT32: u8 = 0x03;
const BIN_POINTER: u8 = 0x04;
const BIN_WIDESTRING: u8 = 0x05;
const BIN_COLOR: u8 = 0x06;
const BIN_UINT64: u8 = 0x07;
const BIN_END: u8 = 0x08;
const BIN_INT64: u8 = 0x0a;

#[derive(Debug, Clone)]
enum VdfValue {
    Map(Vec<(String, VdfValue)>),
    Str(String),
    WStr(String),
    Int32(i32),
    Float32(f32),
    Pointer(i32),
    Color(i32),
    UInt64(u64),
    Int64(i64),
}

#[derive(Debug)]
pub enum SteamShortcutError {
    NoSteamUserdata,
    Io(io::Error),
    Corrupt(&'static str),
    NoExePath,
}

impl fmt::Display for SteamShortcutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SteamShortcutError::NoSteamUserdata => {
                write!(f, "Could not find a Steam userdata directory")
            }
            SteamShortcutError::Io(e) => write!(f, "{e}"),
            SteamShortcutError::Corrupt(msg) => {
                write!(f, "Could not parse existing shortcuts.vdf: {msg}")
            }
            SteamShortcutError::NoExePath => {
                write!(f, "Could not determine Drop's own executable path")
            }
        }
    }
}

impl From<io::Error> for SteamShortcutError {
    fn from(value: io::Error) -> Self {
        SteamShortcutError::Io(value)
    }
}

impl serde::Serialize for SteamShortcutError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

const STEAM_DATA_DIRS: &[&str] = &[
    ".steam/root",
    ".steam/steam",
    ".steam/debian-installation",
    ".local/share/Steam",
    ".local/share/steam",
    ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    ".var/app/com.valvesoftware.Steam/data/Steam",
];

fn find_shortcuts_vdf_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    for dir in STEAM_DATA_DIRS {
        let userdata = home.join(dir).join("userdata");
        let Ok(entries) = fs::read_dir(&userdata) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.chars().all(|c| c.is_ascii_digit()) {
                let config_dir = userdata.join(name).join("config");
                fs::create_dir_all(&config_dir).ok()?;
                return Some(config_dir.join("shortcuts.vdf"));
            }
        }
    }
    None
}

fn parse_cstring(bytes: &[u8], idx: &mut usize) -> Result<String, SteamShortcutError> {
    let start = *idx;
    while bytes.get(*idx).is_some_and(|b| *b != 0) {
        *idx += 1;
    }
    if *idx >= bytes.len() {
        return Err(SteamShortcutError::Corrupt("unterminated string"));
    }
    let s = String::from_utf8_lossy(&bytes[start..*idx]).into_owned();
    *idx += 1;
    Ok(s)
}

fn parse_wcstring(bytes: &[u8], idx: &mut usize) -> Result<String, SteamShortcutError> {
    let start = *idx;
    while *idx + 1 < bytes.len() && (bytes[*idx] != 0 || bytes[*idx + 1] != 0) {
        *idx += 2;
    }
    if *idx + 1 >= bytes.len() {
        return Err(SteamShortcutError::Corrupt("unterminated wide string"));
    }
    let units: Vec<u16> = bytes[start..*idx]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    *idx += 2;
    Ok(String::from_utf16_lossy(&units))
}

fn parse_map(bytes: &[u8], idx: &mut usize) -> Result<Vec<(String, VdfValue)>, SteamShortcutError> {
    let mut entries = Vec::new();
    loop {
        let tag = *bytes
            .get(*idx)
            .ok_or(SteamShortcutError::Corrupt("unexpected end of file"))?;
        *idx += 1;
        if tag == BIN_END {
            break;
        }
        let key = parse_cstring(bytes, idx)?;
        let value = match tag {
            BIN_NONE => VdfValue::Map(parse_map(bytes, idx)?),
            BIN_STRING => VdfValue::Str(parse_cstring(bytes, idx)?),
            BIN_WIDESTRING => VdfValue::WStr(parse_wcstring(bytes, idx)?),
            BIN_INT32 | BIN_POINTER | BIN_COLOR => {
                let raw = bytes
                    .get(*idx..*idx + 4)
                    .ok_or(SteamShortcutError::Corrupt("truncated int32"))?;
                let v = i32::from_le_bytes(raw.try_into().unwrap());
                *idx += 4;
                match tag {
                    BIN_POINTER => VdfValue::Pointer(v),
                    BIN_COLOR => VdfValue::Color(v),
                    _ => VdfValue::Int32(v),
                }
            }
            BIN_UINT64 => {
                let raw = bytes
                    .get(*idx..*idx + 8)
                    .ok_or(SteamShortcutError::Corrupt("truncated uint64"))?;
                *idx += 8;
                VdfValue::UInt64(u64::from_le_bytes(raw.try_into().unwrap()))
            }
            BIN_INT64 => {
                let raw = bytes
                    .get(*idx..*idx + 8)
                    .ok_or(SteamShortcutError::Corrupt("truncated int64"))?;
                *idx += 8;
                VdfValue::Int64(i64::from_le_bytes(raw.try_into().unwrap()))
            }
            BIN_FLOAT32 => {
                let raw = bytes
                    .get(*idx..*idx + 4)
                    .ok_or(SteamShortcutError::Corrupt("truncated float32"))?;
                *idx += 4;
                VdfValue::Float32(f32::from_le_bytes(raw.try_into().unwrap()))
            }
            _ => return Err(SteamShortcutError::Corrupt("unknown value type")),
        };
        entries.push((key, value));
    }
    Ok(entries)
}

fn write_map(out: &mut Vec<u8>, entries: &[(String, VdfValue)]) {
    for (key, value) in entries {
        match value {
            VdfValue::Map(m) => {
                out.push(BIN_NONE);
                out.extend(key.as_bytes());
                out.push(0);
                write_map(out, m);
            }
            VdfValue::Str(s) => {
                out.push(BIN_STRING);
                out.extend(key.as_bytes());
                out.push(0);
                out.extend(s.as_bytes());
                out.push(0);
            }
            VdfValue::WStr(s) => {
                out.push(BIN_WIDESTRING);
                out.extend(key.as_bytes());
                out.push(0);
                for unit in s.encode_utf16() {
                    out.extend(unit.to_le_bytes());
                }
                out.extend([0, 0]);
            }
            VdfValue::Int32(v) => {
                out.push(BIN_INT32);
                out.extend(key.as_bytes());
                out.push(0);
                out.extend(v.to_le_bytes());
            }
            VdfValue::Pointer(v) => {
                out.push(BIN_POINTER);
                out.extend(key.as_bytes());
                out.push(0);
                out.extend(v.to_le_bytes());
            }
            VdfValue::Color(v) => {
                out.push(BIN_COLOR);
                out.extend(key.as_bytes());
                out.push(0);
                out.extend(v.to_le_bytes());
            }
            VdfValue::UInt64(v) => {
                out.push(BIN_UINT64);
                out.extend(key.as_bytes());
                out.push(0);
                out.extend(v.to_le_bytes());
            }
            VdfValue::Int64(v) => {
                out.push(BIN_INT64);
                out.extend(key.as_bytes());
                out.push(0);
                out.extend(v.to_le_bytes());
            }
            VdfValue::Float32(v) => {
                out.push(BIN_FLOAT32);
                out.extend(key.as_bytes());
                out.push(0);
                out.extend(v.to_le_bytes());
            }
        }
    }
    out.push(BIN_END);
}

fn read_shortcut_entries(
    path: &PathBuf,
) -> Result<Vec<Vec<(String, VdfValue)>>, SteamShortcutError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(path)?;
    let mut idx = 0;
    let root = parse_map(&bytes, &mut idx)?;
    for (key, value) in root {
        if key == "shortcuts"
            && let VdfValue::Map(entries) = value
        {
            return Ok(entries
                .into_iter()
                .filter_map(|(_, v)| match v {
                    VdfValue::Map(fields) => Some(fields),
                    _ => None,
                })
                .collect());
        }
    }
    Ok(Vec::new())
}

fn write_shortcut_entries(
    path: &PathBuf,
    entries: Vec<Vec<(String, VdfValue)>>,
) -> Result<(), SteamShortcutError> {
    let indexed: Vec<(String, VdfValue)> = entries
        .into_iter()
        .enumerate()
        .map(|(i, fields)| (i.to_string(), VdfValue::Map(fields)))
        .collect();
    let root = vec![("shortcuts".to_string(), VdfValue::Map(indexed))];
    let mut out = Vec::new();
    write_map(&mut out, &root);
    fs::write(path, out)?;
    Ok(())
}

fn launch_options_for(game_id: &str) -> String {
    format!("drop://launch/{game_id}")
}

fn generate_shortcut_id(exe: &str, app_name: &str) -> i32 {
    let unique_id = format!("{exe}{app_name}");
    let crc = crc32fast::hash(unique_id.as_bytes());
    (crc | 0x8000_0000) as i32
}

fn drop_executable_path() -> Result<String, SteamShortcutError> {
    // Prefer the stable, original .AppImage path over current_exe(), which
    // for an AppImage is a temporary per-launch FUSE mount point that
    // wouldn't exist anymore the next time the shortcut is actually used.
    if let Ok(appimage) = std::env::var("APPIMAGE") {
        return Ok(appimage);
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .ok_or(SteamShortcutError::NoExePath)
}

fn shortcut_matches_game(fields: &[(String, VdfValue)], game_id: &str) -> bool {
    let marker = launch_options_for(game_id);
    fields.iter().any(|(key, value)| {
        key == "LaunchOptions" && matches!(value, VdfValue::Str(s) if s == &marker)
    })
}

pub fn shortcut_exists(game_id: &str) -> Result<bool, SteamShortcutError> {
    let Some(path) = find_shortcuts_vdf_path() else {
        return Err(SteamShortcutError::NoSteamUserdata);
    };
    let entries = read_shortcut_entries(&path)?;
    Ok(entries
        .iter()
        .any(|fields| shortcut_matches_game(fields, game_id)))
}

/// Adds `game_id` to Steam as a non-Steam game shortcut, if it isn't
/// already present. Launching it from Steam runs Drop (or hands off to an
/// already-running instance) with `drop://launch/<game_id>` as an
/// argument/deep link, which starts the game's first launch option.
pub fn add_game_to_steam(game_id: &str, app_name: &str) -> Result<(), SteamShortcutError> {
    let Some(path) = find_shortcuts_vdf_path() else {
        return Err(SteamShortcutError::NoSteamUserdata);
    };

    let mut fields_list = read_shortcut_entries(&path)?;

    if fields_list
        .iter()
        .any(|fields| shortcut_matches_game(fields, game_id))
    {
        info!("Steam shortcut for {game_id} already exists, not adding again");
        return Ok(());
    }

    let exe = drop_executable_path()?;
    let quoted_exe = format!("\"{exe}\"");
    let start_dir = dirs::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    let new_entry = vec![
        (
            "appid".to_string(),
            VdfValue::Int32(generate_shortcut_id(&quoted_exe, app_name)),
        ),
        ("AppName".to_string(), VdfValue::Str(app_name.to_string())),
        ("Exe".to_string(), VdfValue::Str(quoted_exe)),
        (
            "StartDir".to_string(),
            VdfValue::Str(format!("\"{start_dir}\"")),
        ),
        ("icon".to_string(), VdfValue::Str(String::new())),
        (
            "LaunchOptions".to_string(),
            VdfValue::Str(launch_options_for(game_id)),
        ),
        ("IsHidden".to_string(), VdfValue::Int32(0)),
        ("AllowDesktopConfig".to_string(), VdfValue::Int32(1)),
        ("AllowOverlay".to_string(), VdfValue::Int32(1)),
        ("OpenVR".to_string(), VdfValue::Int32(0)),
        ("Devkit".to_string(), VdfValue::Int32(0)),
        ("DevkitOverrideAppID".to_string(), VdfValue::Int32(0)),
        ("LastPlayTime".to_string(), VdfValue::Int32(0)),
    ];
    fields_list.push(new_entry);

    write_shortcut_entries(&path, fields_list)?;
    info!("Added Steam shortcut for {game_id} ({app_name})");
    Ok(())
}

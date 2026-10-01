// Linux-only file: finding the Proton builds installed on this machine.

use std::{
    cmp::Ordering,
    fs::{DirEntry, read_dir, read_to_string},
    io,
    path::PathBuf,
    sync::LazyLock,
};

use database::Database;
use log::warn;
use serde::Serialize;

static SEARCH_PATHS: LazyLock<Vec<String>> = LazyLock::new(|| {
    let mut paths = vec!["/usr/share/steam/compatibilitytools.d/".to_owned()];

    if let Some(home_dir) = std::env::home_dir() {
        paths.push(
            home_dir
                .join(".steam/root/compatibilitytools.d/")
                .to_string_lossy()
                .to_string(),
        );
    }

    paths
});

pub fn read_proton_path(proton_path: PathBuf) -> Result<Option<ProtonPath>, io::Error> {
    let read_dir = read_dir(&proton_path)?.flatten().collect::<Vec<DirEntry>>();
    let has_proton_path = read_dir
        .iter()
        .find(|v| v.file_name().to_string_lossy() == "proton")
        .is_some();
    if !has_proton_path {
        return Ok(None);
    };

    let compat_vdf = read_dir
        .iter()
        .find(|v| v.file_name().to_string_lossy() == "compatibilitytool.vdf");

    let compat_vdf = match compat_vdf {
        Some(v) => v,
        None => return Ok(None),
    };

    let compat_vdf = read_to_string(compat_vdf.path())?;
    let compat_vdf = keyvalues_parser::parse(&compat_vdf)
        .inspect_err(|err| warn!("failed to parse vdf: {:?}", err))
        .map_err(|err| io::Error::other(err.to_string()))?;

    // Function was made with a lot of trial and error
    // Not intended to be readable
    let get_display_name = || -> Option<String> {
        let compat_tools = compat_vdf.value.unwrap_obj();
        let compat_tools = compat_tools.values().next()?.iter().next()?;
        let compat_tools = compat_tools.get_obj().unwrap();
        let compat_tools = compat_tools.values().next()?.iter().next()?.get_obj()?;
        let display_name = compat_tools.get("display_name")?.iter().next()?.get_str()?;
        Some(display_name.to_string())
    };

    if let Some(display_name) = get_display_name() {
        return Ok(Some(ProtonPath {
            path: proton_path.to_string_lossy().to_string(),
            name: display_name,
        }));
    }

    Ok(None)
}

pub fn discover_proton_paths() -> Vec<ProtonPath> {
    let mut results = Vec::new();

    for search_path in &*SEARCH_PATHS {
        let Ok(potential_dirs) = read_dir(search_path) else {
            continue;
        };
        for proton_path in potential_dirs {
            // A single unreadable/broken entry (a dangling symlink, a
            // permissions hiccup, or Steam itself touching this directory
            // mid-scan) shouldn't abort discovery of every other candidate.
            let proton_path = match proton_path {
                Ok(v) => v,
                Err(e) => {
                    warn!("skipping unreadable proton search entry: {e}");
                    continue;
                }
            };
            match read_proton_path(proton_path.path()) {
                Ok(Some(proton)) => results.push(proton),
                Ok(None) => {}
                Err(e) => warn!(
                    "skipping unreadable proton candidate {}: {e}",
                    proton_path.path().display()
                ),
            }
        }
    }

    results
}

// The installed Proton build closest to `name`, for filling in a server's
// recommended Proton at install. Compared case-insensitively against both the
// display name and the folder name: an exact match wins, then the newest
// build starting with it (so "GE-Proton9" picks the newest GE-Proton9-*),
// then the newest build containing it. Takes the database rather than
// locking it, so callers already holding it can use this.
pub fn find_proton_by_name(database: &Database, name: &str) -> Option<ProtonPath> {
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        return None;
    }

    let custom = database
        .applications
        .additional_proton_paths
        .iter()
        .filter_map(|v| read_proton_path(PathBuf::from(v)).ok().flatten());

    discover_proton_paths()
        .into_iter()
        .chain(custom)
        .filter_map(|proton| closeness(&proton, &name).map(|c| (c, proton)))
        .max_by(|(ca, a), (cb, b)| cb.cmp(ca).then_with(|| natural_cmp(&a.name, &b.name)))
        .map(|(_, proton)| proton)
}

// Whether the Proton at `path` is one `name` would match at all, i.e. one
// that could have been picked for it.
pub fn proton_matches_name(path: &str, name: &str) -> bool {
    let name = name.trim().to_lowercase();
    !name.is_empty()
        && read_proton_path(PathBuf::from(path))
            .ok()
            .flatten()
            .is_some_and(|proton| closeness(&proton, &name).is_some())
}

// How well a Proton build matches a lowercased name: 0 for an exact match,
// 1 for a prefix, 2 for containing it, None for no match.
fn closeness(proton: &ProtonPath, name: &str) -> Option<u8> {
    let folder = PathBuf::from(&proton.path)
        .file_name()
        .map(|v| v.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    [proton.name.to_lowercase(), folder]
        .iter()
        .filter_map(|candidate| {
            if candidate == name {
                Some(0)
            } else if candidate.starts_with(name) {
                Some(1)
            } else if candidate.contains(name) {
                Some(2)
            } else {
                None
            }
        })
        .min()
}

// Compares runs of digits by value, so GE-Proton9-20 sorts after GE-Proton9-3.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn chunks(s: &str) -> Vec<(bool, &str)> {
        let mut chunks = Vec::new();
        let mut start = 0;
        for (i, c) in s.char_indices().skip(1) {
            let prev = s[..i]
                .chars()
                .next_back()
                .is_some_and(|p| p.is_ascii_digit());
            if prev != c.is_ascii_digit() {
                chunks.push((prev, &s[start..i]));
                start = i;
            }
        }
        if !s.is_empty() {
            let digit = s[start..].starts_with(|c: char| c.is_ascii_digit());
            chunks.push((digit, &s[start..]));
        }
        chunks
    }

    for (x, y) in chunks(a).into_iter().zip(chunks(b)) {
        let order = match (x, y) {
            ((true, x), (true, y)) => x
                .trim_start_matches('0')
                .len()
                .cmp(&y.trim_start_matches('0').len())
                .then_with(|| x.trim_start_matches('0').cmp(y.trim_start_matches('0'))),
            ((_, x), (_, y)) => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

#[derive(Serialize)]
pub struct ProtonPath {
    pub path: String,
    pub name: String,
}

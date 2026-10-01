//! Fills a game's per-game settings in from the server's recommended Proton
//! settings for the version being installed.
//!
//! A setting counts as the player's own choice once it differs from what
//! Drop filled in. On an update, a setting the player never touched follows
//! the new version's recommendation, while one they changed is left alone.
//! "What Drop filled in" is worked out from the previous version's stored
//! recommendation, so nothing extra has to be tracked per setting.
//!
//! The preferred Proton is a name, filled in as the installed build closest
//! to it (see client::proton::find_proton_by_name).

use database::{Database, ProtonDefaults, UserConfiguration};

pub fn apply_proton_defaults(
    database: &Database,
    configuration: &mut UserConfiguration,
    previous: Option<&ProtonDefaults>,
    new: Option<&ProtonDefaults>,
) {
    #[cfg(target_os = "linux")]
    apply_preferred_proton(database, configuration, previous, new);
    #[cfg(not(target_os = "linux"))]
    let _ = database;

    fn follow<T: PartialEq>(current: &mut T, filled_before: T, recommended: T) {
        if *current == filled_before {
            *current = recommended;
        }
    }

    // What each setting is when a recommendation is applied: the server
    // says whether DXVK/Esync/Fsync should be on, the client stores whether
    // they're disabled.
    let disabled = |v: Option<bool>| v.map(|on| !on).unwrap_or(false);
    let defaults = ProtonDefaults::default();
    let previous = previous.unwrap_or(&defaults);
    let new = new.unwrap_or(&defaults);

    follow(
        &mut configuration.disable_dxvk,
        disabled(previous.dxvk),
        disabled(new.dxvk),
    );
    follow(
        &mut configuration.disable_esync,
        disabled(previous.esync),
        disabled(new.esync),
    );
    follow(
        &mut configuration.disable_fsync,
        disabled(previous.fsync),
        disabled(new.fsync),
    );
    follow(
        &mut configuration.locale,
        previous.locale.clone(),
        new.locale.clone(),
    );
    follow(
        &mut configuration.extra_env_vars,
        previous.extra_env_vars.clone().unwrap_or_default(),
        new.extra_env_vars.clone().unwrap_or_default(),
    );
}

#[cfg(target_os = "linux")]
fn apply_preferred_proton(
    database: &Database,
    configuration: &mut UserConfiguration,
    previous: Option<&ProtonDefaults>,
    new: Option<&ProtonDefaults>,
) {
    use client::proton::{find_proton_by_name, proton_matches_name};

    let previous_name = previous.and_then(|v| v.proton_name.as_deref());
    let new_name = new.and_then(|v| v.proton_name.as_deref());

    // Unset, or still the build Drop picked for the previous recommendation.
    let filled_by_drop = match (&configuration.override_proton_path, previous_name) {
        (None, _) => true,
        (Some(path), Some(name)) => proton_matches_name(path, name),
        (Some(_), None) => false,
    };
    if !filled_by_drop {
        return;
    }

    match new_name {
        Some(name) => match find_proton_by_name(database, name) {
            Some(proton) => configuration.override_proton_path = Some(proton.path),
            // Nothing installed matches: keep what's there rather than
            // dropping a working pick back to the global default.
            None => log::info!("no installed Proton matches the recommended {name:?}"),
        },
        // The server no longer recommends one, so the build Drop picked
        // goes back to following the global default.
        None => configuration.override_proton_path = None,
    }
}

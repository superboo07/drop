use std::collections::HashMap;

use database::DownloadableMetadata;
use download_manager::DOWNLOAD_MANAGER;
use games::downloads::file_plan::{ConflictChoice, PendingConflicts, answer_pending, list_pending};

#[tauri::command]
pub async fn pause_downloads() {
    DOWNLOAD_MANAGER.pause_downloads().await;
}

#[tauri::command]
pub async fn resume_downloads() {
    DOWNLOAD_MANAGER.resume_downloads().await;
}

#[tauri::command]
pub async fn move_download_in_queue(old_index: usize, new_index: usize) {
    DOWNLOAD_MANAGER.rearrange(old_index, new_index).await;
}

#[tauri::command]
pub async fn cancel_game(meta: DownloadableMetadata) {
    DOWNLOAD_MANAGER.cancel(meta).await;
}

/// Downloads waiting on the player to say what happens to files they
/// changed, for a window that opens after the prompt was first sent.
#[tauri::command]
pub fn fetch_file_conflicts() -> Vec<PendingConflicts> {
    list_pending()
}

#[tauri::command]
pub fn resolve_file_conflicts(
    game_id: String,
    choices: HashMap<String, ConflictChoice>,
) -> Result<(), String> {
    if answer_pending(&game_id, choices) {
        Ok(())
    } else {
        Err("This download isn't waiting for an answer any more.".to_string())
    }
}

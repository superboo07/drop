use std::collections::HashSet;
use std::sync::nonpoison::Mutex;

use async_trait::async_trait;
use client::{app_state::AppState, app_status::AppStatus};
use database::{PendingPlaytimeSession, borrow_db_checked, borrow_db_mut_checked};
use log::warn;
use remote::{
    auth::generate_authorization_header, requests::generate_url, utils::DROP_APP_HANDLE,
    utils::DROP_CLIENT_ASYNC,
};
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::scheduler::ScheduleTask;

pub struct PlaytimeSyncer;

impl PlaytimeSyncer {
    pub fn new() -> Self {
        PlaytimeSyncer
    }
}

pub struct PlaytimeCheckpointer;

impl PlaytimeCheckpointer {
    pub fn new() -> Self {
        PlaytimeCheckpointer
    }
}

// Flushes a playtime chunk for every currently-running game every minute
// (see ProcessManager::checkpoint_running_sessions), instead of relying
// solely on on_process_finish recording the whole session at exit. That's
// what makes a game killed alongside Drop itself - e.g. Steam killing the
// entire process tree when the user hits "Stop" on a non-Steam shortcut,
// which never gives on_process_finish a chance to run - only lose up to a
// minute of playtime instead of the whole session. Local-only, so it runs
// far more often than PlaytimeSyncer's network sync.
#[async_trait]
impl ScheduleTask for PlaytimeCheckpointer {
    fn timeframe(&mut self) -> usize {
        1
    }

    async fn call(&mut self) -> Result<(), anyhow::Error> {
        ::process::PROCESS_MANAGER.lock().checkpoint_running_sessions();
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionPayload {
    id: String,
    game_id: String,
    seconds: u64,
    started_at: chrono::DateTime<chrono::Utc>,
    ended_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
struct SyncBody {
    sessions: Vec<SessionPayload>,
}

#[derive(Deserialize)]
struct SyncResponse {
    accepted: Vec<String>,
}

// Syncs playtime sessions recorded by ProcessManager::on_process_finish
// (process_manager.rs) to the server. Nothing is removed from the local
// pending queue until the server explicitly confirms it in its response -
// this is what makes retries (a lost response, an app restart mid-sync)
// safe: the server's session-id primary key means a resend of an
// already-applied session is a harmless no-op, never a double-count.
#[async_trait]
impl ScheduleTask for PlaytimeSyncer {
    fn timeframe(&mut self) -> usize {
        5
    }

    async fn call(&mut self) -> Result<(), anyhow::Error> {
        let app_handle = DROP_APP_HANDLE.lock().await;
        let app_handle = app_handle
            .as_ref()
            .ok_or(anyhow::anyhow!("playtime sync task ran before setup"))?;
        {
            let state = app_handle.state::<Mutex<AppState>>();
            let state_lock = state.lock();
            if state_lock.status == AppStatus::Offline {
                return Ok(());
            }
        }

        let pending: Vec<PendingPlaytimeSession> = {
            let db = borrow_db_checked();
            db.applications.pending_playtime_sessions.clone()
        };
        if pending.is_empty() {
            return Ok(());
        }

        let body = SyncBody {
            sessions: pending
                .iter()
                .map(|s| SessionPayload {
                    id: s.id.clone(),
                    game_id: s.game_id.clone(),
                    seconds: s.seconds,
                    started_at: s.started_at,
                    ended_at: s.ended_at,
                })
                .collect(),
        };

        let url = generate_url(&["/api/v1/client/playtime"], &[])?;
        let response = DROP_CLIENT_ASYNC
            .post(url)
            .header("Authorization", generate_authorization_header())
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            warn!("playtime sync failed: {}", response.status());
            return Ok(()); // keep everything queued, retry next cycle
        }

        let accepted: SyncResponse = response.json().await?;
        let accepted: HashSet<String> = accepted.accepted.into_iter().collect();

        let mut db = borrow_db_mut_checked();
        db.applications
            .pending_playtime_sessions
            .retain(|s| !accepted.contains(&s.id));

        Ok(())
    }
}

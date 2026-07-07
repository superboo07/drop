use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize, Eq, PartialEq)]
pub enum AppStatus {
    NotConfigured,
    Offline,
    ServerError,
    SignedOut,
    SignedIn,
    SignedInNeedsReauth,
    ServerUnavailable,
}

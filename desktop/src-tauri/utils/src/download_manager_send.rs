/// Sends a signal to the download manager, logging (rather than panicking) if
/// the receiving end has gone away.
///
/// A closed channel is a normal, reachable state - the manage_queue loop
/// returns on `Finish` and drops the receiver during shutdown, so anything
/// still finishing up (a download thread emitting `Completed`, a spawned
/// progress update) will find nobody listening. Panicking there used to take
/// the whole app down with it, because the desktop client builds with
/// `panic = "abort"`.
#[macro_export]
macro_rules! send {
    ($download_manager:expr, $signal:expr) => {
        if let Err(e) = $download_manager.send($signal).await {
            ::log::warn!(
                "failed to send signal {} to the download manager: {}",
                stringify!($signal),
                e
            );
        }
    };
}

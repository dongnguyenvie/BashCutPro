//! Flush the recovery journal when the window is ended by SIGINT / SIGTERM (FX12). The journal
//! is debounced (~1 s), so without this the edits of the last second before `kill -TERM` were
//! lost. The cleanup runs on the signal thread, which cannot reach the session: it asks the UI
//! thread (wakes it with a repaint) and waits at most [`WAIT`]; the next [`poll`] calls
//! `Persistence::flush` (engine API, no upstream edit) and answers. The journal keeps
//! `cleanExit: false`, so the next launch offers it as after a crash (`bashcut/agent-connect.md`).

use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use crate::FilmcraftApp;

/// How long the signal thread waits for the UI thread before the process exits anyway (a busy
/// or stalled UI thread must not keep a logout waiting).
pub const WAIT: Duration = Duration::from_secs(3);

/// Set by the signal cleanup: answered once the journal is written.
static PENDING: Mutex<Option<Sender<()>>> = Mutex::new(None);

/// Called every pass from [`super::logic`]: registers the cleanup once (when the session has a
/// journal) and serves a pending flush request.
pub fn poll(app: &FilmcraftApp, ctx: &egui::Context) {
    let Some(p) = app.session.persistence.as_ref() else { return };
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        let ctx = ctx.clone();
        bashcut_dirs::signal::at_terminate("recovery", move || request(&ctx));
    });
    if let Some(done) = PENDING.lock().unwrap_or_else(PoisonError::into_inner).take() {
        // `sync_persistence` already ran after the last command; this writes what is still debounced
        p.flush();
        let _ = done.send(());
    }
}

/// The signal side: ask the UI thread to flush and wait for it (at most [`WAIT`]).
fn request(ctx: &egui::Context) {
    let (tx, rx) = channel();
    *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
    ctx.request_repaint();
    if rx.recv_timeout(WAIT).is_err() {
        log::warn!("recovery journal not flushed before exit: the window did not answer within {WAIT:?}");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "test ticket: SIGTERM within the 1 s journal debounce still leaves the last edit in snapshot.fcproj"]
    fn term_flushes_the_debounced_journal() {}
}

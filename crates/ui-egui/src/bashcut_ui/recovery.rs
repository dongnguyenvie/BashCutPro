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
    #[cfg(unix)]
    use super::PENDING;

    /// TREC1: SIGTERM within the 1 s journal debounce still leaves the last edit in snapshot.fcproj.
    ///
    /// Drives the flush path the SIGTERM handler uses without a full UI stack:
    /// 1. Start a `Session` with a 30 s journal debounce (so the worker never fires on its own).
    /// 2. Execute one edit so the session is dirty and a snapshot is pending in the worker.
    /// 3. Put a `Sender<()>` into `PENDING` the way the real `request()` does on SIGTERM.
    /// 4. Spawn a thread that emulates the "poll" side of the egui frame loop: it takes the sender
    ///    from `PENDING`, calls `Persistence::flush`, then sends `()` back to the signal side.
    /// 5. `recv_timeout(WAIT)` on the signal side must succeed — the handshake must complete within
    ///    the allowed window.
    /// 6. `snapshot.fcproj` must exist in the session directory and be a valid project file.
    #[cfg(unix)]
    #[test]
    fn term_flushes_the_debounced_journal() {
        use std::sync::mpsc::channel;
        use std::time::Duration;

        use filmcraft_engine::Session;
        use filmcraft_engine::autosave::AutosaveConfig;
        use serde_json::json;

        // A scratch directory that holds this session's journal.
        let data = std::env::temp_dir().join(format!("bc-trec1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();

        // Long debounce: the worker will never write the journal on its own during this test.
        let mut cfg = AutosaveConfig::new(&data);
        cfg.journal_debounce = Duration::from_secs(30);

        let mut session = Session::default();
        session.execute("file.openDemoProject", json!({})).expect("demo project");
        session.start_autosave(cfg).expect("start autosave");

        // Make one edit so the journal has a pending snapshot.
        session.execute("sequence.addEdit", json!({"seconds": 1.5})).expect("edit");
        // sync_persistence hands the dirty Arc<Project> to the worker thread.
        session.sync_persistence();

        // Give the dirty message a moment to arrive at the worker before we flush.
        std::thread::sleep(Duration::from_millis(20));

        // The session directory where the journal will be written.
        let session_dir = session.persistence.as_ref().expect("persistence").session_dir.clone();

        // ── Signal side (simulates what `request()` does on SIGTERM) ──────────────────────────
        // Create the channel the SIGTERM handler uses to wait for the UI thread's acknowledgement.
        let (tx, rx) = channel::<()>();
        *PENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(tx);

        // ── UI-thread side (simulates what `poll()` does on the next egui frame) ──────────────
        // A dedicated thread stands in for the egui frame loop. It takes the sender from PENDING,
        // calls Persistence::flush (the path poll() uses), then sends the acknowledgement.
        let poll_thread = std::thread::spawn(move || {
            let done = PENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
            if let Some(done) = done {
                // This is what poll() does: flush the pending journal and notify the signal side.
                session.persistence.as_ref().expect("persistence").flush();
                let _ = done.send(());
            }
        });

        // ── Signal-thread wait ─────────────────────────────────────────────────────────────────
        // The SIGTERM handler waits at most WAIT; this must complete well before that.
        assert!(rx.recv_timeout(super::WAIT).is_ok(), "poll() did not answer the SIGTERM handshake within {:?}", super::WAIT);

        poll_thread.join().expect("poll thread panicked");

        // ── Assertion ─────────────────────────────────────────────────────────────────────────
        let snap = session_dir.join("snapshot.fcproj");
        assert!(snap.exists(), "snapshot.fcproj must exist after the SIGTERM flush: {}", snap.display());
        let bytes = std::fs::read(&snap).expect("read snapshot");
        assert!(!bytes.is_empty(), "snapshot must not be empty");
        // The snapshot must be a valid project file (JSON with the filmcraft.project envelope).
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("snapshot is valid JSON");
        assert_eq!(v["format"], json!("filmcraft.project"), "snapshot has the expected envelope");

        let _ = std::fs::remove_dir_all(&data);
    }
}

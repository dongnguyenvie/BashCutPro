//! BashCutPro: one undo step for a command made of many upstream commands (#199).
//!
//! Counting `undo.len()` before and after does not work: the history keeps at most `limit` entries (200), so a long
//! run pushes its own first entries and the steps before it out of the front, and the count folds the wrong ones
//! (`bashcut.captions.wordStyle` lost the previous step and left a stray "Character Style"). [`UndoGroup`] lifts the
//! limit while the steps run, then replaces everything they pushed with one entry holding the project as it was
//! before. Unlike `Session::edit_group` it keeps a part-way result (and folds it) when a step fails.

use std::sync::Arc;

use filmcraft_project::Project;

use crate::Session;

pub(crate) struct UndoGroup {
    project: Arc<Project>,
    undo_len: usize,
    limit: usize,
}

impl UndoGroup {
    /// Start a group: no step folds into an edit made before it, and none is pushed out of the history.
    pub(crate) fn start(s: &mut Session) -> UndoGroup {
        let g = UndoGroup { project: s.project.clone(), undo_len: s.history.undo.len(), limit: s.history.limit };
        s.history.merge_key = None;
        s.history.limit = usize::MAX;
        g
    }

    /// Drop the group: the project and the undo history go back to how they were at [`UndoGroup::start`] (the
    /// caller restores the rest of the session).
    pub(crate) fn cancel(self, s: &mut Session) {
        s.project = self.project;
        s.history.undo.truncate(self.undo_len);
        s.history.merge_key = None;
        s.history.limit = self.limit;
    }

    /// End the group: the entries its steps pushed become one labelled `label`. Returns whether there is one.
    pub(crate) fn finish(self, s: &mut Session, label: &str) -> bool {
        s.history.limit = self.limit;
        s.history.merge_key = None;
        let pushed = s.history.undo.len().saturating_sub(self.undo_len);
        if s.history.undo.len() < self.undo_len {
            // a step reset the history (a project was opened): nothing to fold into
            return pushed > 0;
        }
        if pushed == 0 && Arc::ptr_eq(&s.project, &self.project) {
            return false;
        }
        s.history.undo.truncate(self.undo_len);
        s.history.undo.push((label.to_string(), self.project));
        let over = s.history.undo.len().saturating_sub(s.history.limit.max(1));
        if over > 0 {
            s.history.undo.drain(..over);
        }
        true
    }
}

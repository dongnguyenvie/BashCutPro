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
    /// The undo entries before the group (whole-project `Arc`s: cheap), put back by [`UndoGroup::cancel`] even
    /// when a step replaced the history.
    undo: Vec<(String, Arc<Project>)>,
    limit: usize,
}

impl UndoGroup {
    /// Start a group: no step folds into an edit made before it, and none is pushed out of the history.
    pub(crate) fn start(s: &mut Session) -> UndoGroup {
        let g = UndoGroup { project: s.project.clone(), undo: s.history.undo.clone(), limit: s.history.limit };
        s.history.merge_key = None;
        s.history.limit = usize::MAX;
        g
    }

    /// Drop the group: the project and the undo history go back to how they were at [`UndoGroup::start`] (the
    /// caller restores the rest of the session).
    pub(crate) fn cancel(self, s: &mut Session) {
        s.project = self.project;
        s.history.undo = self.undo;
        s.history.merge_key = None;
        s.history.limit = self.limit;
    }

    /// End the group: the entries its steps pushed become one labelled `label`. Returns whether there is one.
    pub(crate) fn finish(self, s: &mut Session, label: &str) -> bool {
        // a step replaced the history (a project was opened or made): every reset sets the limit back from the
        // usize::MAX of `start`. The new history stays and there is nothing to fold into: an entry holding the
        // project from before would undo the open.
        let reset = s.history.limit != usize::MAX;
        s.history.limit = self.limit;
        s.history.merge_key = None;
        if reset {
            return false;
        }
        let undo_len = self.undo.len();
        let pushed = s.history.undo.len().saturating_sub(undo_len);
        if pushed == 0 && Arc::ptr_eq(&s.project, &self.project) {
            return false;
        }
        s.history.undo.truncate(undo_len);
        s.history.undo.push((label.to_string(), self.project));
        let over = s.history.undo.len().saturating_sub(s.history.limit.max(1));
        if over > 0 {
            s.history.undo.drain(..over);
        }
        true
    }
}

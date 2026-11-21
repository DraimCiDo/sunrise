//! The transaction journal fake: what a host keeps on each transaction it opens.

use crate::item::transaction::TransactionFrame;
use crate::{ServerHost, TransactionContext, TransactionScope};

// no wire: in-process testing harness state, not a mod boundary payload.
/// A test host's transaction journal: stores the starting state at each depth and records each
/// close in order. Implement a test host's scope by delegating to [`Self::with_scope`].
/// Cloning copies the history, not the open frames: a cloned host is a separate journal owner.
#[derive(Debug)]
#[non_exhaustive]
pub struct FakeJournal<S> {
    kept: Vec<(TransactionFrame, S)>,
    closed: Vec<(usize, bool)>,
}

impl<S> Clone for FakeJournal<S> {
    fn clone(&self) -> Self {
        Self {
            kept: Vec::new(),
            closed: self.closed.clone(),
        }
    }
}

impl<S> Default for FakeJournal<S> {
    fn default() -> Self {
        Self {
            kept: Vec::new(),
            closed: Vec::new(),
        }
    }
}

struct Scope<'a, H, S> {
    host: &'a mut H,
    journal: fn(&mut H) -> &mut FakeJournal<S>,
    restore: fn(&mut H, S),
    frame: TransactionFrame,
    commit: bool,
}

impl<H, S> Drop for Scope<'_, H, S> {
    fn drop(&mut self) {
        let journal = (self.journal)(self.host);
        if let Some((_, before)) = journal.kept.pop_if(|(frame, _)| *frame == self.frame) {
            journal.closed.push((self.frame.depth() + 1, self.commit));
            if !self.commit {
                (self.restore)(self.host, before);
            }
        }
    }
}

impl<S> FakeJournal<S> {
    /// Lends a single journal loan, returning whether the callback ran. `New` snapshots the
    /// owner before invoking it and restores on refusal or unwind; `Join` only lends the current
    /// context, if any. Test hosts using this helper represent the world phase.
    pub fn with_scope<H: ServerHost>(
        host: &mut H,
        mode: TransactionScope,
        callback: &mut dyn for<'loan> FnMut(
            &'loan mut dyn ServerHost,
            &'loan TransactionContext,
        ) -> bool,
        journal: fn(&mut H) -> &mut Self,
        snapshot: impl FnOnce(&H) -> S,
        restore: fn(&mut H, S),
    ) -> bool {
        match mode {
            TransactionScope::New => {
                let before = snapshot(host);
                let owner = journal(host);
                let frame = TransactionFrame::fresh(owner.depth());
                owner.kept.push((frame, before));
                let mut scope = Scope {
                    host,
                    journal,
                    restore,
                    frame,
                    commit: false,
                };
                scope.commit = frame.lend(|tx| callback(&mut *scope.host, tx));
                true
            }
            TransactionScope::Join => {
                let Some(&(frame, _)) = journal(host).kept.last() else {
                    return false;
                };
                frame.lend(|tx| callback(host, tx));
                true
            }
        }
    }

    /// Whether `tx` belongs to this journal, including any open ancestor of the top frame.
    pub fn holds(&self, tx: &TransactionContext) -> bool {
        self.kept
            .get(tx.depth())
            .is_some_and(|(frame, _)| frame.matches(tx))
    }

    /// Whether `expected` is the top frame. `None` matches an empty journal.
    pub fn matches(&self, expected: Option<&TransactionContext>) -> bool {
        match (self.kept.last(), expected) {
            (Some((frame, _)), Some(tx)) => frame.matches(tx),
            (None, None) => true,
            _ => false,
        }
    }

    /// How many transactions are open.
    pub fn depth(&self) -> usize {
        self.kept.len()
    }

    /// Each transaction closed, in order: the depth it was open at (1 for the outermost), and
    /// whether it committed.
    pub fn closed(&self) -> &[(usize, bool)] {
        &self.closed
    }
}

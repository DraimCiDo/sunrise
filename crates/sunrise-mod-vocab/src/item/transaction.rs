//! `host.transaction` (#41 §7): NeoForge 26.3's `Transaction`. Changes made through the
//! capability helpers inside one are kept by the game and made final only when the outermost
//! transaction commits; an abort puts back everything its closure changed.
use crate::ServerHost;
use std::{
    fmt,
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

// no wire: only the host can lend this thread-bound authority over a live journal.
/// A loan of write authority in one open transaction, as NeoForge's `TransactionContext`.
/// Pass this loan and the accompanying server host to handler, item-place and world fluid
/// writes. It cannot close the transaction, be encoded as payload data, or outlive the
/// callback it was lent to. A live loan from another host is not authority on this host.
/// When opening a nested savepoint, use the context that its own callback receives.
// exhaustive: its fields are private, so mods cannot build or destructure it.
pub struct TransactionContext {
    frame: TransactionFrame,
    _thread: PhantomData<Rc<()>>,
}

impl TransactionContext {
    /// The nesting depth at which this context was lent: zero at the root, plus one per
    /// enclosing transaction. Depth alone cannot identify a frame or journal.
    pub fn depth(&self) -> usize {
        self.frame.depth
    }
}

impl fmt::Debug for TransactionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransactionContext")
            .field("depth", &self.depth())
            .finish_non_exhaustive()
    }
}

// no wire: process-local identity of a journal frame.
/// A key for the host that owns a journal frame, not a write capability or close handle.
#[derive(Clone, Copy, Debug)]
pub struct TransactionFrame {
    namespace: &'static AtomicU64,
    sequence: u64,
    depth: usize,
}

impl PartialEq for TransactionFrame {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.namespace, other.namespace) && self.sequence == other.sequence
    }
}

impl Eq for TransactionFrame {}

impl TransactionFrame {
    /// Mints a fresh identity at `depth`. Separately linked copies use different namespaces.
    pub fn fresh(depth: usize) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("transaction frame identities exhausted");
        Self {
            namespace: &NEXT,
            sequence,
            depth,
        }
    }

    /// The depth at which this frame was installed.
    pub fn depth(self) -> usize {
        self.depth
    }

    /// Whether the loan came from this journal frame.
    pub fn matches(self, context: &TransactionContext) -> bool {
        self == context.frame
    }

    /// Lends write authority without handing out an owned context.
    pub fn lend<T>(self, f: impl for<'loan> FnOnce(&'loan TransactionContext) -> T) -> T {
        f(&TransactionContext {
            frame: self,
            _thread: PhantomData,
        })
    }
}

// no wire: observes the process-local transaction journal.
/// A read-only observation of the host's journal phase (`Transaction.Lifecycle`), returned by
/// [`transaction_lifecycle`](crate::prelude::ModStateServerHost::transaction_lifecycle).
/// It is not a write or close capability or a Wire payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransactionLifecycle {
    /// No transaction is open.
    None,
    /// A transaction is open and none is closing.
    Open,
    /// A nested transaction is closing: committing into the one around it, or aborting.
    Closing,
    /// The outermost transaction is closing. On commit, effects such as `contents_changed`
    /// and block updates run after its frame has been removed; they cannot join that frame.
    RootClosing,
}

// no wire: selects how the host lends access to its process-local journal.
// exhaustive: the host either opens a savepoint or lends the active frame.
/// Whether to open a savepoint or join the current transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionScope {
    /// Open a new transaction, nested if one is already open.
    New,
    /// Join the innermost transaction without opening or closing anything.
    Join,
}

// no wire: a local refusal to lend transaction authority, not a mod boundary payload.
/// This host cannot lend a transaction context in the current phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransactionUnavailable;

// no wire: the error belongs to a local closure and can contain any Rust type.
// exhaustive: the closure either did not run or ran and returned an error.
/// Why [`HostTransaction::transaction`] did not commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionError<E> {
    /// The host could not open a transaction. The closure was not called.
    Unavailable,
    /// The closure returned this error. Its writes were rolled back.
    Aborted(E),
}

/// Transactions are available only on the logical server. Both the host and the context are
/// lent to the callback; neither can escape it.
///
/// ```
/// use mod_api::{
///     BlockEntityError, BlockEntityRef, HostTransaction, ServerHost, TransactionError,
///     TransactionUnavailable,
/// };
/// fn write(host: &mut dyn ServerHost, block: BlockEntityRef) {
///     let _: Result<(), TransactionError<BlockEntityError>> = host.transaction(|host, _tx| {
///         block.set_changed(host)?;
///         Ok(())
///     });
/// }
/// fn active_depth(host: &mut dyn ServerHost) -> Result<Option<usize>, TransactionUnavailable> {
///     host.join_transaction(|_, tx| tx.depth())
/// }
/// ```
/// ```compile_fail
/// use mod_api::{BlockEntityError, BlockEntityRef, Host, HostTransaction, TransactionError};
/// fn write(host: &mut dyn Host, block: BlockEntityRef) {
///     let _: Result<(), TransactionError<BlockEntityError>> = host.transaction(|host, _tx| {
///         block.set_changed(host)?;
///         Ok(())
///     });
/// }
/// ```
/// ```compile_fail
/// use mod_api::{ClientHost, HostTransaction, TransactionError};
/// fn client(host: &mut dyn ClientHost) {
///     let _: Result<(), TransactionError<()>> = host.transaction(|_, _| Ok(()));
/// }
/// ```
/// ```compile_fail
/// use mod_api::{HostTransaction, ServerHost, TransactionContext, TransactionError};
/// fn escape(host: &mut dyn ServerHost) {
///     let _: Result<&TransactionContext, TransactionError<()>> = host.transaction(|_, tx| Ok(tx));
/// }
/// ```
/// ```compile_fail
/// use mod_api::{HostTransaction, ServerHost, TransactionContext, TransactionUnavailable};
/// fn stash(host: &mut dyn ServerHost) {
///     let mut saved: Option<&TransactionContext> = None;
///     let _: Result<Option<()>, TransactionUnavailable> = host.join_transaction(|_, tx| {
///         saved = Some(tx);
///     });
///     let _ = saved;
/// }
/// ```
/// ```compile_fail
/// use mod_api::{HostTransaction, ServerHost, TransactionError};
/// fn close(host: &mut dyn ServerHost) {
///     let _: Result<(), TransactionError<()>> = host.transaction(|host, tx| {
///         host.transaction_close(tx, true);
///         Ok(())
///     });
/// }
/// ```
pub trait HostTransaction: ServerHost {
    /// Runs `f` in a transaction, nested in the open one if any (`Transaction.openRoot` and
    /// `open`). `Ok` commits into the transaction around it or, at the outermost, for good,
    /// with the world told of each change once. `Err` aborts, restoring every storage it wrote.
    /// An unwind out of `f` aborts this transaction before reaching the caller; the transaction
    /// around it stays open.
    ///
    /// An error returned by `f` becomes [`TransactionError::Aborted`]. If a transaction cannot
    /// be opened, [`TransactionError::Unavailable`] means `f` did not run.
    ///
    /// `f` gets a loan of the server host and [`TransactionContext`] to write in. Only the
    /// host's owning scope ends the transaction.
    ///
    /// Covered: block entities (item slots, saved compound, custom name and attachments), cauldrons,
    /// players' inventories and cursors, entities' inventories
    /// ([`ItemPlace::EntitySlot`](crate::ItemPlace::EntitySlot)) and item entities' stacks.
    fn transaction<T, E>(
        &mut self,
        f: impl for<'loan> FnOnce(&'loan mut dyn ServerHost, &'loan TransactionContext) -> Result<T, E>,
    ) -> Result<T, TransactionError<E>> {
        let mut f = Some(f);
        let mut result = None;
        self.with_transaction_scope(TransactionScope::New, &mut |host, tx| {
            let Some(f) = f.take() else { return false };
            let outcome = f(host, tx);
            let commit = outcome.is_ok();
            result = Some(outcome);
            commit
        })
        .map_err(|TransactionUnavailable| TransactionError::Unavailable)?;
        result
            .ok_or(TransactionError::Unavailable)?
            .map_err(TransactionError::Aborted)
    }

    /// Lends the innermost open action's context, or returns `Ok(None)` without calling `f`
    /// if there is no open action. Returns [`TransactionUnavailable`] outside world access
    /// or when the host does not support scoped transactions.
    ///
    /// Joining opens and closes nothing. Writes take effect in the opener's journal, and
    /// `f`'s return value does not commit or abort it, even when that value is an `Err`.
    /// Use [`Self::transaction`] for a new savepoint that can roll back independently.
    fn join_transaction<T>(
        &mut self,
        f: impl for<'loan> FnOnce(&'loan mut dyn ServerHost, &'loan TransactionContext) -> T,
    ) -> Result<Option<T>, TransactionUnavailable> {
        let mut f = Some(f);
        let mut result = None;
        self.with_transaction_scope(TransactionScope::Join, &mut |host, tx| {
            if let Some(f) = f.take() {
                result = Some(f(host, tx));
            }
            // A join never decides the action's outcome.
            false
        })?;
        Ok(result)
    }
}

impl<H: ServerHost + ?Sized> HostTransaction for H {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHost;
    use crate::{ITEMS, Identifier, ItemResource, ItemStack, ResourceHandler, prelude::ItemsHost};

    const AT: [i32; 3] = [0, 64, 0];

    fn fake_with_stone() -> (FakeHost, ItemResource) {
        let mut fake = FakeHost::new();
        let stone = Identifier::parse("minecraft:stone").unwrap();
        fake.registries.resolve_items([stone.clone()]).unwrap();
        let stone = ItemStack::new(fake.item_key(&stone).unwrap(), 1).unwrap();
        let resource = ItemResource::of(&stone);
        fake.level
            .block_entity(AT)
            .with_items(1)
            .items
            .as_mut()
            .unwrap()
            .slots[0] = Some(stone);
        (fake, resource)
    }

    #[test]
    fn a_live_context_from_another_host_cannot_write() {
        let (mut owner, resource) = fake_with_stone();
        let mut other = owner.clone();
        let level = owner.level.level();
        let items = level.capability(&mut owner, &ITEMS, AT, None).unwrap();

        other
            .transaction(|other, foreign| {
                owner
                    .transaction(|host, own| {
                        assert_eq!((foreign.depth(), own.depth()), (0, 0));
                        assert!(!host.transaction_matches(Some(foreign)));
                        assert_eq!(items.extract_at(host, foreign, 0, &resource, 1), 0);
                        assert_eq!(items.amount(host, 0), 1);
                        assert_eq!(items.extract_at(host, own, 0, &resource, 1), 1);
                        Ok::<_, ()>(())
                    })
                    .unwrap();
                assert!(other.transaction_matches(Some(foreign)));
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(items.amount(&mut owner, 0), 0);
        assert_eq!(items.amount(&mut other, 0), 1);
        assert_eq!(owner.journal.closed(), [(1, true)]);
        assert_eq!(other.journal.closed(), [(1, true)]);
    }

    #[test]
    fn a_transaction_commits_on_ok_and_aborts_on_err_and_nests() {
        let mut fake = FakeHost::new();
        let host: &mut dyn ServerHost = &mut fake;
        let outer = host.transaction(|host, _| {
            let inner = host.transaction(|_, _| Err::<(), _>("refused"));
            assert_eq!(inner, Err(TransactionError::Aborted("refused")));
            let committed = host.transaction(|_, _| Ok::<_, ()>(7)).unwrap();
            Ok::<_, ()>(committed)
        });
        assert_eq!(outer, Ok(7));
        assert_eq!(fake.journal.closed(), [(2, false), (2, true), (1, true)]);
        assert_eq!(fake.journal.depth(), 0);
    }

    #[test]
    fn joining_lends_the_open_frame_and_does_not_control_rollback() {
        let (mut fake, resource) = fake_with_stone();
        let level = fake.level.level();
        let items = level.capability(&mut fake, &ITEMS, AT, None).unwrap();
        let host: &mut dyn ServerHost = &mut fake;

        assert_eq!(
            host.join_transaction::<()>(|_, _| panic!("no open action")),
            Ok(None)
        );
        let result = host.transaction(|host, outer| {
            let joined = host.join_transaction(|host, tx| {
                assert_eq!(tx.depth(), 0);
                assert!(host.transaction_matches(Some(outer)));
                assert!(host.transaction_matches(Some(tx)));
                assert_eq!(items.extract_at(host, tx, 0, &resource, 1), 1);
                Err::<(), _>("only the opener can abort")
            });
            assert_eq!(joined, Ok(Some(Err("only the opener can abort"))));
            assert_eq!(items.amount(host, 0), 0);
            Err::<(), _>("leave the item in place")
        });
        assert_eq!(
            result,
            Err(TransactionError::Aborted("leave the item in place"))
        );
        assert_eq!(items.amount(host, 0), 1);
        assert_eq!(fake.journal.closed(), [(1, false)]);
    }

    #[test]
    fn a_panic_aborts_exactly_the_transaction_it_leaves() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let mut fake = FakeHost::new();
        let host: &mut dyn ServerHost = &mut fake;
        // A child's panic caught inside the root: the child aborts, the root still commits.
        let root = host.transaction(|host, _| {
            let child = catch_unwind(AssertUnwindSafe(|| {
                host.transaction(|_, _| -> Result<(), ()> { panic!("a broken handler") })
            }));
            assert!(child.is_err());
            Ok::<_, ()>(())
        });
        assert_eq!(root, Ok(()));
        // A root's panic caught outside it: the root aborts before the catch sees anything.
        let host: &mut dyn ServerHost = &mut fake;
        let caught = catch_unwind(AssertUnwindSafe(|| {
            host.transaction(|_, _| -> Result<(), ()> { panic!("a broken handler") })
        }));
        assert!(caught.is_err());
        assert_eq!(fake.journal.closed(), [(2, false), (1, true), (1, false)]);
        assert_eq!(fake.journal.depth(), 0);
    }
}

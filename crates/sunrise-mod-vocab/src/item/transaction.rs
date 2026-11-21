//! `host.transaction` ( §7): NeoForge 26.3's `Transaction`. Changes made through the
//! capability helpers inside one are kept by the game and made final only when the outermost
//! transaction commits; an abort puts back everything its closure changed.
use crate::ServerHost;

/// An open transaction, as NeoForge's `TransactionContext`: what every write of a handler or a
/// place takes. `host.transaction` gets one from the game and lends it to its closure alone.
///
/// It lets its holder write in the transaction, not end it: ending one takes its
/// [`TransactionSeal`], which only whoever opened it holds. The authority is the game's, not
/// this value's: it names one transaction the game opened by a token that cannot be guessed in
/// practice (a randomly keyed 64-bit hash), and the game checks it on every write. A write
/// given a transaction that is not open changes nothing, so a value decoded from bytes or kept
/// past its closure cannot write outside the journal.
// exhaustive: its field is private, so mods cannot build or destructure it
#[derive(Debug, PartialEq, Eq, sunrise_mod_api_macros::Wire)]
pub struct TransactionContext(pub(crate) u64);

/// What ends a transaction:
/// [`transaction_open`](crate::prelude::ModStateServerHost::transaction_open) gives it to whoever opened
/// the transaction (`host.transaction`'s guard), and
/// [`transaction_close`](crate::prelude::ModStateServerHost::transaction_close) takes it. A
/// write handler lent only the [`TransactionContext`] cannot end the transaction around it, the game's
/// own (a hopper's move) or another mod's.
///
/// It carries the transaction's write token and a second token, as unguessable, that only
/// closing checks.
///
/// ```compile_fail
/// use mod_api::{HostTransaction, ServerHost};
/// fn handler(host: &mut dyn ServerHost) {
///     let _ = host.transaction(|host, tx| -> Result<(), ()> {
///         host.transaction_close(tx, true);
///         Ok(())
///     });
/// }
/// ```
// exhaustive: its fields are private, so mods cannot build or destructure it
#[derive(Debug, PartialEq, Eq, sunrise_mod_api_macros::Wire)]
pub struct TransactionSeal {
    pub(crate) tx: TransactionContext,
    pub(crate) close: u64,
}

impl TransactionSeal {
    /// The transaction this seal ends, to write in it.
    pub fn transaction(&self) -> &TransactionContext {
        &self.tx
    }
}

/// The seal of the transaction the game opened with write token `token` and close token
/// `close`, for its raw open helper.
// hidden: the game's side of the seal, re-exported through `__private`.
#[doc(hidden)]
pub fn transaction_seal(token: u64, close: u64) -> TransactionSeal {
    TransactionSeal {
        tx: TransactionContext(token),
        close,
    }
}

/// The transaction the game opened with write token `token`, for its
/// `transaction_current` answer: a write token alone, never a seal.
// hidden: the game's side of the seal, re-exported through `__private`.
#[doc(hidden)]
pub fn transaction_of(token: u64) -> TransactionContext {
    TransactionContext(token)
}

/// Where the transaction system is (`Transaction.Lifecycle`), as
/// [`transaction_lifecycle`](crate::prelude::ModStateServerHost::transaction_lifecycle) reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[derive(sunrise_mod_api_macros::Wire)]
pub enum TransactionLifecycle {
    /// No transaction is open.
    None,
    /// A transaction is open and none is closing.
    Open,
    /// A nested transaction is closing: committing into the one around it, or aborting.
    Closing,
    /// The outermost transaction is closing: on a commit, the game is telling the world of its
    /// changes (`setChanged`, `contents_changed`, block updates), so a write now lands outside
    /// the transaction that just ended.
    RootClosing,
}

/// The write token `tx` names, for the game's checks.
// hidden: the game's side of the seal, re-exported through `__private`.
#[doc(hidden)]
pub fn transaction_token(tx: &TransactionContext) -> u64 {
    tx.0
}

/// The write and close tokens `seal` carries, for the game's checks.
// hidden: the game's side of the seal, re-exported through `__private`.
#[doc(hidden)]
pub fn seal_tokens(seal: &TransactionSeal) -> (u64, u64) {
    (seal.tx.0, seal.close)
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

/// Transactions are available only on the logical server and preserve the receiver type.
///
/// ```
/// use mod_api::{BlockEntityError, BlockEntityRef, HostTransaction, ServerHost, TransactionError};
/// fn write(host: &mut dyn ServerHost, block: BlockEntityRef) {
///     let _: Result<(), TransactionError<BlockEntityError>> = host.transaction(|host, _tx| {
///         block.set_changed(host)?;
///         Ok(())
///     });
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
pub trait HostTransaction: ServerHost {
    /// Runs `f` in a transaction, nested in the open one if any (`Transaction.openRoot` and
    /// `open`): `Ok` commits it (`commit`), into the transaction around it or, at the
    /// outermost, for good, with the world told of each change once; `Err` aborts it
    /// (`close` without `commit`), putting every storage it wrote back as it was. A panic
    /// unwinding out of `f` aborts this transaction as it passes, so whoever catches it sees
    /// nothing `f` changed, and the transaction around it stays open. A transaction `f` opened
    /// itself and left open is aborted first, when this one closes.
    ///
    /// An error returned by `f` becomes [`TransactionError::Aborted`]. If a transaction cannot
    /// be opened, [`TransactionError::Unavailable`] means `f` did not run.
    ///
    /// `f` gets the [`TransactionContext`] to write in, never its [`TransactionSeal`]: only this
    /// guard ends the transaction.
    ///
    /// Covered: block entities (item slots, saved compound, custom name and attachments), cauldrons,
    /// players' inventories and cursors, entities' inventories
    /// ([`ItemPlace::EntitySlot`](crate::ItemPlace::EntitySlot)) and item entities' stacks.
    fn transaction<T, E>(
        &mut self,
        f: impl FnOnce(&mut Self, &TransactionContext) -> Result<T, E>,
    ) -> Result<T, TransactionError<E>>;

    /// Lends the innermost open action's context, or returns `Ok(None)` without calling `f`
    /// if there is no open action. Returns [`TransactionUnavailable`] outside world access.
    ///
    /// Joining opens and closes nothing. Writes take effect in the opener's journal, and
    /// `f`'s return value does not commit or abort it. Use [`Self::transaction`] for a new
    /// savepoint that can roll back independently.
    fn join_transaction<T>(
        &mut self,
        f: impl for<'loan> FnOnce(&'loan mut Self, &'loan TransactionContext) -> T,
    ) -> Result<Option<T>, TransactionUnavailable> {
        use crate::world::phase::{LoadAccess, access_allowed};
        if !access_allowed("join_transaction", LoadAccess::WorldOnly) {
            return Err(TransactionUnavailable);
        }
        Ok(self.transaction_current().map(|context| f(self, &context)))
    }
}

impl<H: ServerHost + ?Sized> HostTransaction for H {
    fn transaction<T, E>(
        &mut self,
        f: impl FnOnce(&mut Self, &TransactionContext) -> Result<T, E>,
    ) -> Result<T, TransactionError<E>> {
        run_transaction(self, f).map_err(TransactionError::Aborted)
    }
}

/// `host.transaction` for a host of a known type, which `f` gets back: the game's own moves (a
/// hopper's) run in one through this.
pub fn run_transaction<H: ServerHost + ?Sized, T, E>(
    host: &mut H,
    f: impl FnOnce(&mut H, &TransactionContext) -> Result<T, E>,
) -> Result<T, E> {
    /// Closes its transaction when dropped: committed when `f` gave `Ok`, else aborted, as on
    /// a panic unwinding past it.
    struct Open<'a, H: ServerHost + ?Sized> {
        host: &'a mut H,
        seal: TransactionSeal,
        commit: bool,
    }
    impl<H: ServerHost + ?Sized> Drop for Open<'_, H> {
        fn drop(&mut self) {
            self.host.transaction_close(&self.seal, self.commit);
        }
    }
    let seal = host.transaction_open();
    let mut open = Open {
        host,
        seal,
        commit: false,
    };
    let result = f(&mut *open.host, open.seal.transaction());
    open.commit = result.is_ok();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHost;

    /// A seal built from bytes: from the write token a handler is lent and any guessed close
    /// token, outside any transaction and inside nested ones.
    fn forged(token: u64, close: u64) -> TransactionSeal {
        let mut bytes = token.to_le_bytes().to_vec();
        bytes.extend(close.to_le_bytes());
        crate::wire::Reader::decode(&bytes).unwrap()
    }

    #[test]
    fn a_handler_lent_the_transaction_cannot_end_it_or_the_one_around() {
        assert!(
            crate::wire::Reader::decode::<TransactionSeal>(&7u64.to_le_bytes()).is_err(),
            "no seal from a write token alone"
        );
        let mut fake = FakeHost::new();
        let host: &mut dyn ServerHost = &mut fake;
        host.transaction_close(&forged(1, 1), true);
        let outer = host.transaction(|host, outer_tx| {
            let outer_token = transaction_token(outer_tx);
            host.transaction(|host, tx| {
                // The handler knows both write tokens, not the close tokens.
                for token in [outer_token, transaction_token(tx)] {
                    for close in [0, 1, token, !token, u64::MAX] {
                        host.transaction_close(&forged(token, close), true);
                    }
                }
                Ok::<_, ()>(())
            })
            .unwrap();
            Err::<(), ()>(())
        });
        assert_eq!(outer, Err(TransactionError::Aborted(())));
        assert_eq!(
            fake.journal.closed(),
            [(2, true), (1, false)],
            "only each guard closed its own, and the caller's abort stood"
        );
        assert_eq!(fake.journal.depth(), 0);
    }

    #[test]
    fn a_transaction_left_open_inside_aborts_when_its_caller_closes() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let mut fake = FakeHost::new();
        let host: &mut dyn ServerHost = &mut fake;
        // The caller aborts: the child it left open aborts first.
        let outer = host.transaction(|host, _| {
            let _left_open = host.transaction_open();
            Err::<(), ()>(())
        });
        assert_eq!(outer, Err(TransactionError::Aborted(())));
        assert_eq!(fake.journal.closed(), [(2, false), (1, false)]);
        assert_eq!(fake.journal.depth(), 0);
        // The caller commits: the abandoned child still aborts.
        let host: &mut dyn ServerHost = &mut fake;
        let outer = host.transaction(|host, _| {
            let _left_open = host.transaction_open();
            Ok::<_, ()>(())
        });
        assert_eq!(outer, Ok(()));
        assert_eq!(fake.journal.closed()[2..], [(2, false), (1, true)]);
        // A panic: the rollback is done before whoever catches it looks.
        let host: &mut dyn ServerHost = &mut fake;
        let caught = catch_unwind(AssertUnwindSafe(|| {
            host.transaction(|host, _| -> Result<(), ()> {
                let _left_open = host.transaction_open();
                panic!("a broken handler")
            })
        }));
        assert!(caught.is_err());
        assert_eq!(fake.journal.closed()[4..], [(2, false), (1, false)]);
        assert_eq!(fake.journal.depth(), 0);
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
        use crate::{
            ITEMS, Identifier, ItemResource, ItemStack, ResourceHandler, prelude::ItemsHost,
        };
        let mut fake = FakeHost::new();
        let level = fake.level.level();
        let at = [0, 64, 0];
        let stone = Identifier::parse("minecraft:stone").unwrap();
        fake.registries.resolve_items([stone.clone()]).unwrap();
        let stone = ItemStack::new(fake.item_key(&stone).unwrap(), 1).unwrap();
        let resource = ItemResource::of(&stone);
        fake.level.block_entity(at).with_items(1);
        fake.level.block_entity(at).items.as_mut().unwrap().slots[0] = Some(stone);
        let items = level.capability(&mut fake, &ITEMS, at, None).unwrap();
        let host: &mut dyn ServerHost = &mut fake;

        assert_eq!(
            host.join_transaction::<()>(|_, _| panic!("no open action")),
            Ok(None)
        );
        let result = host.transaction(|host, _| {
            let joined =
                host.join_transaction(|host, tx| items.extract_at(host, tx, 0, &resource, 1));
            assert_eq!(joined, Ok(Some(1)));
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

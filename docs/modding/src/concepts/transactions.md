# Transactions

Import `HostTransaction` to group server-side writes in one action. Return `Ok` from the
closure to commit, or `Err` to roll back. For example, move one item between two places
only if both writes succeed:

```rust
use mod_api::{HostTransaction, ItemPlace, ItemResource, ServerHost, TransactionError};

fn move_one(
    host: &mut dyn ServerHost,
    from: &ItemPlace,
    to: &ItemPlace,
    item: &ItemResource,
) -> Result<(), TransactionError<()>> {
    host.transaction(|host, tx| {
        if from.extract(host, tx, item, 1) != 1 || to.insert(host, tx, item, 1) != 1 {
            return Err(());
        }
        Ok(())
    })
}
```

The closure returns `Result<T, E>`; `host.transaction` returns `Result<T, TransactionError<E>>`:

- `Ok(value)`: the transaction committed.
- `Err(TransactionError::Aborted(error))`: the closure returned this error and its journaled writes rolled back.
- `Err(TransactionError::Unavailable)`: the host could not open a transaction and the closure did not run.

When the caller uses a different error type, map both variants before propagating with `?`.

## Commit and rollback

- Writes apply immediately, so subsequent steps in the transaction see them.
- The game snapshots each storage before its first write. Handlers use the game's
  journal rather than keeping their own.
- A nested commit merges into its parent. Only the outermost commit makes storage
  changes final and publishes deferred engine-managed container effects.
- An abort restores recorded storage without publishing those commit effects.
- Direct writes by mods to their own storage and rollback restores issue no notices.
- Explicit changed/comparator/neighbour/data-send notifications run when issued,
  including inside a transaction, and survive abort.
- A panic unwinding out of the closure aborts its transaction. The parent remains
  open if you catch the panic inside the parent closure.

The journal covers writes through capability handlers to these storages:

| Storage | Recorded state |
|---|---|
| Block entity | Item slots, saved compound, custom name and attachments |
| Hopper | Cooldown |
| Cauldron | Block state |
| Composter | Block state and composter-local random stream; fill feedback, game events and maturation scheduling wait for outer commit |
| World block set, removal, and fluid pickup and placement | Block state, and a block entity the new block removes |
| Player | Inventory slots, armour, offhand, body, saddle, cursor, ender chest and persistent data, for host and guest players |
| Entity inventory | Chest or hopper minecart, chest boat, chested donkey, mule or llama inventory |
| Item entity | Stack; an emptied entity remains until the outermost commit removes it |

At the outermost commit, default `ITEMS` and item `STORAGE` writes to vanilla containers
and engine-managed mod slots publish their implicit change notifications.
Other deferred effects include jukebox playback, cauldron and composter block updates and
item drops, including items placed at a block, thrown by a player or entity, or split off an
item entity. Explicit mod-issued world notifications are not deferred with these effects.

A world pickup or placement of fluid (`level.pickup_fluid`, `level.place_fluid`) writes
its block at the call and the rest at the outermost commit; an aborted one leaves the world
unchanged, where NeoForge's may not. `level.play_fluid_effects` queues a fluid's sound and
game event the same way: they play only at the outermost commit and are dropped on abort,
unlike `trigger_sound_and_game_event`, which acts at once. The `block_drops` listeners of a
replaced block see it as it was, and a cancelled drop keeps the replacement.

`LevelRef::set_block` and `LevelRef::remove_block` expose the tentative block state
right away; block updates and replacement effects wait for the outermost commit.
`LevelRef::destroy_block` returns `Ok(None)` while a transaction is open, before
running loot, experience or break effects.

Other direct writes are not journaled: direct item-entity stack writes,
`LivingRef::set_equipment`, and the armor-stand `slot_set` paths take
effect immediately, even inside a transaction. Equipment sounds and equip/unequip
events from `LivingRef::set_equipment` also happen immediately. An abort does not
undo these writes or effects. Use journaled capability handlers when you need rollback.

## Spawning and removing entities

Entities a transaction spawns or removes follow the same commit and rollback rules:

- A spawn builds the entity at the call: its handle works, its attachments and Parts exist,
  `entity_constructing` has run and selectors find it. Its `entity_join_level` does not run
  until the outermost commit, in the order the entities were added, before any removal
  of the transaction takes effect. A listener reads the entity at the commit, not during the
  call. A join a listener cancels at the commit means the entity was never announced and the
  handle the spawn returned reads stale from then on.
- An abort takes out every entity the transaction spawned, with its attachments and saved data, and
  tells no mod: neither `entity_join_level` nor `entity_leave_level` runs. An entity id is
  never reused, so the aborted entity's id stays burnt and a stale handle never names a later entity.
- A removal (`EntityRef::discard`, a kill) marks the entity at the call: its handle and selectors
  stop finding it, and a mob reads as removed (it is flagged at the call; any other entity is
  listed as removed until the commit has run its removal, joins told by then included).
  Passengers dismount, the leave
  is told and the other effects of removal wait for the outermost commit; an abort puts the
  entity back as it was and tells no one. An entity spawned and removed in the same
  transaction is never announced, so it never leaves.
- A spawn is refused, inside a transaction as outside one, once the entity ids run out.
- A transaction covers the entities of the level it was opened in; with transactions of several levels open (a command run `in` another level can open one), an entity belongs to the innermost one of its own level. A command it runs in another
  level (`execute in`) spawns or removes there as it would outside a transaction when no
  transaction is open in that level: the join and the leave are told at once, and an abort does
  not take the entity back. With one open there, the innermost of that level journals it, as above. A join listener that
  spawns, removes or commits a transaction while a commit is telling its joins is told about its
  own entities after the commit's batch, in the order it asked.
- Travel between levels is not journaled: an entity that arrives in another level during an aborted
  transaction stays where it arrived.

## Transaction authority

The closure borrows a `&mut dyn ServerHost` and a `&TransactionContext` together. Pass both
down to capability, item-place and world fluid writes. You cannot obtain an owned context,
return the borrow from the closure, save it for later, or encode it as a payload. Even if two hosts
have open roots at the same depth, a context lent by one is not authority on the other.

If a hook or Part already runs inside an action and has no context parameter, use
`host.join_transaction(|host, tx| …)`. It lends the current frame and returns `Ok(Some(value))`
with the callback's result. `Ok(None)` means no action was open; `Err(TransactionUnavailable)`
means the host could not lend access. Neither calls the callback. Joining does not open,
close or roll back anything by itself, even if the callback returns an error. The opener
decides the outcome. Use `host.transaction` for an independent savepoint, and use the fresh
context lent to that closure for its writes. Move helpers can also open their own savepoints:
a failed helper can undo its attempt without undoing the action around it.

## Inspecting a transaction

| Method | Result |
|---|---|
| `tx.depth()` | `usize`: zero for the root, plus one for each level of nesting |
| `host.transaction_matches(Some(tx))` | Whether this loan names the current frame on this host |
| `host.transaction_matches(None)` | Whether this host has no open frame |
| `host.transaction_lifecycle()` | A read-only observation of the host's journal phase |

Depth is not a frame identity. Neither depth nor the observed `TransactionLifecycle` grants
write or close authority, and `TransactionLifecycle` is not a Wire payload.

| `TransactionLifecycle` variant | Meaning |
|---|---|
| `None` | No transaction is open |
| `Open` | A transaction is open and none is closing |
| `Closing` | A nested transaction is committing into its parent or aborting |
| `RootClosing` | The outermost transaction is closing |

Implicit engine-managed container notices run during `RootClosing`, outside the transaction
whose commit triggered them. A listener cannot join that closed transaction. Explicit mod notices
issued during a transaction run during `Open`.

## Entity inventory slots

`ItemPlace::EntitySlot { entity, slot }` addresses an entity inventory slot, like
NeoForge's `Capabilities.Item.ENTITY`. Read and write it like any other item place.
Reading or writing a minecart's slots unpacks its loot table without a player,
as vanilla's `getChestVehicleItem` does.

The player `set_inventory_slot`, `set_ender_chest_slot`, `give_stack` and
`set_persistent_data_key` writes use the open transaction journal. Aborting restores
the old values; equipment effects and given-stack overflow wait for root commit.

These writes reach a player in another level than the hook's: their inventory and data
change at once, wherever they are. What would land in the world at them does not happen
there: `give_stack` returns what did not fit instead of dropping it (drop it from a hook in
the player's own level if you want), `drop_stack` returns the stack, `destroy_block` breaks
nothing, and no pickup or equip sound plays.

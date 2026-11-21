//! Shared queried parameters and NeoForge conditions for both loot evaluators.
use super::LootStack;
use std::{cell::RefCell, rc::Rc};
pub use sunrise_core::UNKNOWN_LOOT_TABLE;

mod declarations;
pub use declarations::condition;

/// Parameters shared unchanged by all nested tables. The first id set wins.
#[derive(Default)]
pub struct LootQuery {
    id: RefCell<Option<String>>,
    /// The stack whose data is being queried. No existing engine caller sets it.
    pub queried_stack: Option<LootStack>,
    /// Tool retained by the world evaluator and its nested tables.
    pub tool: Option<LootStack>,
}
impl LootQuery {
    /// An unset query with stack parameters retained for every nested table.
    pub fn new(queried_stack: Option<LootStack>, tool: Option<LootStack>) -> Self {
        Self {
            id: RefCell::new(None),
            queried_stack,
            tool,
        }
    }
    /// Set once, either by the caller or after the root completes.
    pub fn set_id(&self, id: &str) {
        self.id.borrow_mut().get_or_insert_with(|| id.to_owned());
    }
    /// The stored id, if a caller or completed root supplied one.
    pub fn id(&self) -> Option<String> {
        self.id.borrow().clone()
    }
    /// Unset contexts always report the unknown-table fallback.
    pub fn table_id(&self) -> String {
        self.id().unwrap_or_else(|| UNKNOWN_LOOT_TABLE.into())
    }
    /// Set-once id completion, called by each evaluator's `roll_root` after
    /// all nested tables. Nested raw rolls never call this.
    pub fn finish(&self, id: Option<&str>) {
        self.set_id(id.unwrap_or(UNKNOWN_LOOT_TABLE));
    }
}
thread_local! { static QUERY: RefCell<Option<Rc<LootQuery>>> = const { RefCell::new(None) }; }
/// Supply parameters and inspect the root's completed queried id. Restores on unwind.
pub fn with_query<T>(query: Rc<LootQuery>, roll: impl FnOnce() -> T) -> T {
    struct Restore(Option<Rc<LootQuery>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            QUERY.with(|q| *q.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(QUERY.with(|q| q.replace(Some(query))));
    roll()
}
/// A fresh context unless a caller explicitly supplied query parameters.
pub fn current_query() -> Rc<LootQuery> {
    QUERY.with(|q| q.borrow().clone()).unwrap_or_default()
}

//! Static serializer resources adapted to the owning server loot generation.
#[path = "global_loot_modifiers/declarations.rs"]
mod declarations;
use super::api_values::{TryIntoApi, TryIntoEngine};
pub(super) use declarations::{contributed, files};
use mod_api::{GlobalLootModifier, GlobalLootModifierSerializer, Identifier, LootModifierError};
use std::{collections::HashMap, sync::Arc};
use sunrise_world::{
    loot::{
        self,
        global_modifiers::{GlobalLootModifiers, ModifierApply, ModifierFile, ModifierRoll},
    },
    native_load_context::LoadContext,
};

pub(super) struct Serializers(HashMap<String, Arc<dyn GlobalLootModifierSerializer>>);
impl Serializers {
    pub(super) fn capture() -> Self {
        Self(
            mod_api::__private::global_loot_modifier_serializer_declarations()
                .into_iter()
                .collect(),
        )
    }
    pub(super) fn prepare(
        &self,
        files: Vec<ModifierFile>,
        conditions: &sunrise_world::conditions::LoadConditions,
        load: &LoadContext,
    ) -> Result<
        (
            Arc<GlobalLootModifiers>,
            Arc<mod_api::GlobalLootModifierManager>,
        ),
        String,
    > {
        use mod_api::__private::game::GlobalLootModifierManagerNew;
        let instances = std::cell::RefCell::new(HashMap::new());
        let generation = Arc::new(GlobalLootModifiers::load(
            files,
            conditions,
            |id, kind, data| {
                let (decoded, instance) = self.decode(kind, data, load)?;
                instances.borrow_mut().insert(id.to_owned(), instance);
                Ok(decoded)
            },
            |error| log::error!("{error}"),
        ));
        let mut instances = instances.into_inner();
        let entries = generation
            .ids()
            .map(|id| {
                let key = Identifier::parse(id).map_err(|error| error.to_string())?;
                let instance = instances
                    .remove(id)
                    .ok_or_else(|| format!("prepared modifier {id} missing its API resource"))?;
                Ok((key, instance))
            })
            .collect::<Result<_, String>>()?;
        let manager = Arc::new(mod_api::GlobalLootModifierManager::prepared(
            entries,
            generation.clone(),
        ));
        Ok((generation, manager))
    }
}
struct Adapter(Arc<dyn GlobalLootModifier>);
impl ModifierApply for Adapter {
    fn apply(
        &self,
        roll: &mut dyn ModifierRoll,
        generated: &[loot::LootStack],
    ) -> Result<Vec<loot::LootStack>, String> {
        let snapshot = roll
            .facts()
            .try_into_api()
            .map_err(|error| format!("context: {error:?}"))?;
        let generated = generated
            .iter()
            .map(TryIntoApi::try_into_api)
            .collect::<Result<_, _>>()
            .map_err(|error| format!("generated: {error:?}"))?;
        let mut context = RollAdapter { roll, snapshot };
        self.0
            .apply(&mut context, generated)
            .map_err(|error| error.to_string())?
            .iter()
            .map(|stack| {
                stack
                    .try_into_engine(&())
                    .map_err(|error| format!("output: {error:?}"))
            })
            .collect()
    }
}
struct RollAdapter<'a> {
    roll: &'a mut dyn ModifierRoll,
    snapshot: mod_api::LootContext,
}
impl mod_api::LootModifierContext for RollAdapter<'_> {
    fn snapshot(&self) -> mod_api::LootContext {
        self.snapshot.clone()
    }
    fn random(&mut self) -> &mut dyn mod_api::RandomSource {
        self
    }
    fn roll_raw(
        &mut self,
        table: &Identifier,
    ) -> Result<Vec<mod_api::LootStack>, LootModifierError> {
        self.roll
            .roll_raw(&table.to_string())
            .map_err(|error| LootModifierError::new("roll_raw", error))?
            .iter()
            .map(|stack| {
                stack
                    .try_into_api()
                    .map_err(|error| LootModifierError::new("roll_raw", format!("{error:?}")))
            })
            .collect()
    }
}
impl mod_api::RandomSource for RollAdapter<'_> {
    fn next_int(&mut self) -> i32 {
        self.roll.next_int()
    }
    fn next_int_bound(&mut self, bound: i32) -> i32 {
        self.roll.next_int_bound(bound)
    }
    fn next_long(&mut self) -> i64 {
        self.roll.next_long()
    }
    fn next_float(&mut self) -> f32 {
        self.roll.next_float()
    }
    fn next_double(&mut self) -> f64 {
        self.roll.next_double()
    }
    fn next_boolean(&mut self) -> bool {
        self.roll.next_boolean()
    }
    fn next_gaussian(&mut self) -> f64 {
        self.roll.next_gaussian()
    }
}

/// The reload retains its own generation, never a global live manager. Initial
/// world opening binds the snapshot after its tables exist; reloads bind before
/// any listener apply. A failed preparation simply drops this unbound candidate.
pub(super) fn register_anchor(
    registry: &mut mod_api::ServerListenerRegistry,
) -> Result<Arc<mod_api::GlobalLootModifierManager>, mod_api::ReloadOrderError> {
    use mod_api::{__private::game::ServerReloadListenersOrder, ServerReloadListeners};
    let generation = Arc::new(mod_api::GlobalLootModifierManager::default());
    let id = mod_api::GlobalLootModifierManager::key().id().clone();
    let preceding = registry.get_last_vanilla_listener();
    registry.add_retained(
        id.clone(),
        mod_api::FileToIdConverter::json("loot_modifiers").unwrap(),
        generation.clone(),
        generation.clone(),
    )?;
    registry.add_dependency(preceding, id.clone())?;
    registry.set_last_vanilla_listener(id);
    Ok(generation)
}
pub(super) fn bind_retained(
    resources: &dyn mod_api::ServerReloadResources,
    generation: &mod_api::GlobalLootModifierManager,
) {
    use mod_api::__private::game::GlobalLootModifierManagerNew;
    if let Some(retained) = resources.listener(&mod_api::GlobalLootModifierManager::key()) {
        retained.bind_from(generation);
    }
}

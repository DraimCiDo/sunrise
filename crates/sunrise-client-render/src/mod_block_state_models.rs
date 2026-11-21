//! The block-state definition types native mods register (NeoForge's `RegisterBlockStateModels`),
//! kept once at start, and the hook point that asks a type's mod to pick each state's model when a
//! resource load reads a blockstate file naming it (`neoforge:definition_type`).
use mod_api::{Answered, BlockStateModelPick, HookId, Identifier, RegistrationContext};
use serde_json::Value;
use std::{collections::BTreeMap, sync::RwLock};
use sunrise_assets::blockstate::{Variant, bind::Properties};
use sunrise_core::nbt::Tag;

static TYPES: RwLock<BTreeMap<String, &'static str>> = RwLock::new(BTreeMap::new());

/// Keep the successful registrant for each definition type, in dispatch order. A later
/// registration replaces that type's binding, as in `LateBoundIdMapper.put`.
pub fn register(definitions: Vec<Answered<String>>) {
    let mut types = TYPES.write().unwrap_or_else(|e| e.into_inner());
    for Answered {
        answer: id, mod_id, ..
    } in definitions
    {
        if Identifier::parse_namespaced(&id).is_none() {
            log::warn!("[{mod_id}] Invalid block state definition type: {id:?}");
            continue;
        }
        types.insert(id, mod_id);
    }
}

/// Hook point for native mods (docs/modding/src/how-to/native-mods.md, declared in `hooks/` with
/// the game's): a blockstate file whose definition type a mod registered is read
/// (`CustomBlockModelDefinition.instantiate`). `config` is the file's JSON object, `states` the
/// block's states in registry order; the mod that owns `definition_type` pushes its picks.
#[inline(never)]
pub fn native_instantiate_block_state_definition(
    context: &RegistrationContext,
    mod_id: &str,
    definition_type: &str,
    block: &Identifier,
    config: &Tag,
    states: &[std::collections::BTreeMap<String, String>],
    models: &mut Vec<BlockStateModelPick>,
) {
    if !mod_api::__private::listened(HookId::InstantiateBlockStateDefinition) {
        return;
    }
    mod_api::dispatch_instantiate_block_state_definition(
        context,
        mod_id,
        definition_type,
        block,
        config,
        states,
        models,
    );
}

/// Each state's variant, by index into `states`, as the mod owning definition type `kind` picks
/// them for `block` from the file's JSON `config`. It runs on the resource loader, as a
/// registration. An error says why the file is refused; the caller names the file.
pub fn instantiate(
    kind: &str,
    block: &str,
    config: &Value,
    states: &[Properties],
) -> Result<Vec<Option<Variant>>, String> {
    use mod_api::__private::game::RegistrationContextNew;
    let owner = TYPES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(kind)
        .copied();
    let Some(owner) = owner else {
        return Err(format!("unknown block state definition type {kind}"));
    };
    let block = Identifier::parse(block).map_err(|e| format!("block {block}: {e}"))?;
    let config = sunrise_core::json_nbt::json_to_tag(config)
        .map_err(|e| format!("definition type {kind}: its JSON doesn't convert to NBT: {e}"))?;
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
    let mut picks = Vec::new();
    native_instantiate_block_state_definition(
        &RegistrationContext::new(),
        owner,
        kind,
        &block,
        &config,
        states,
        &mut picks,
    );
    picked(kind, states.len(), picks)
}

/// The picks a type's mod answered, one slot per state: refused whole by a `Refuse`, a pick of a
/// state the block lacks or of one picked already, a model id that doesn't read, or no pick at all.
fn picked(
    kind: &str,
    count: usize,
    picks: Vec<BlockStateModelPick>,
) -> Result<Vec<Option<Variant>>, String> {
    let mut models = vec![None; count];
    for pick in picks {
        match pick {
            BlockStateModelPick::Refuse { reason } => {
                return Err(format!("definition type {kind} refused it: {reason}"));
            }
            BlockStateModelPick::Model {
                state,
                model,
                turns,
                uvlock,
            } => {
                let slot = usize::try_from(state)
                    .ok()
                    .and_then(|state| models.get_mut(state))
                    .ok_or_else(|| {
                        format!(
                            "definition type {kind} picked state {state}, but the block has \
                             {count} states"
                        )
                    })?;
                let variant = Variant::new(&model, turns, uvlock)
                    .map_err(|e| format!("definition type {kind} picked model {model:?}: {e}"))?;
                if slot.replace(variant).is_some() {
                    return Err(format!("definition type {kind} picked state {state} twice"));
                }
            }
        }
    }
    if models.iter().all(Option::is_none) {
        return Err(format!("definition type {kind} picked no model"));
    }
    Ok(models)
}

#[cfg(test)]
#[path = "mod_block_state_models_tests.rs"]
mod tests;

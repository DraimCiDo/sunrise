//! stable-line: decodes-other-data
//! Native condition registration and reload lifecycle.
use super::{candidate_registries, candidate_registry_files};
use crate::game::{Game, functions, mods, server_reload};
use mod_api::__private::game::RegistrationContextNew;
use std::{collections::BTreeMap, sync::Arc};

pub(in crate::game) type ConditionTable =
    Arc<BTreeMap<mod_api::Identifier, (String, Arc<dyn mod_api::LoadCondition>)>>;

impl Game<'_> {
    pub(in crate::game) fn begin_condition_reload(
        &mut self,
        stack: &functions::PackStack,
        resources: &std::path::Path,
    ) -> Result<server_reload::PreparedServerReload, mod_api::__private::reload::ReloadStageError>
    {
        use mod_api::__private::reload::ReloadStageError;
        if self.mods.load_condition_candidate.is_none() {
            let inventory = sunrise_mod_package::enabled::EnabledMods::read(resources)
                .map_err(|error| ReloadStageError::DataSource(error.to_string()))?;
            self.mods.load_condition_candidate = Some(
                conditions_for_mods(&self.mods, &inventory)
                    .map_err(ReloadStageError::DataSource)?,
            );
        }
        self.prepare_server_reload(stack)
    }
    pub(in crate::game) fn prepare_pack_condition_reload(
        &mut self,
        stack: &functions::PackStack,
    ) -> Result<server_reload::PreparedServerReload, mod_api::__private::reload::ReloadStageError>
    {
        self.mods.load_condition_candidate = None;
        self.begin_condition_reload(stack, &self.options.resources.clone())
    }

    pub(in crate::game) fn stage_condition_inventory(
        &mut self,
    ) -> sunrise_client::Result<Option<sunrise_assets::resource_source::ResourceRoot>> {
        self.protect_condition_root();
        self.mods.load_condition_candidate = None;
        let inventory = sunrise_mod_package::enabled::EnabledMods::read(&self.options.resources)?;
        self.mods.load_condition_candidate = Some(conditions_for_mods(&self.mods, &inventory)?);
        self.stage_enabled_inventory()
    }

    pub(in crate::game) fn protect_condition_root(&mut self) {
        self.mods.active_condition_root = Some(sunrise_client::mod_content::retain_root(
            &self.options.resources,
        ));
    }

    pub(in crate::game) fn condition_root_needs_reload(&self) -> bool {
        self.mods
            .load_condition_candidate
            .as_ref()
            .is_some_and(|view| view.has_registered())
            && (!self.mods.selected.is_empty()
                || !self.mods.dev.is_empty()
                || !self.mods.native.packages().is_empty())
    }

    /// The conditions of a load from `root` over the candidate pack `files`.
    pub(in crate::game) fn registered_load_conditions(
        &self,
        inventory: &sunrise_mod_package::enabled::EnabledMods,
        stack: &functions::PackStack,
        root: &std::path::Path,
        files: &[(String, Vec<Vec<u8>>)],
    ) -> Result<sunrise_world::conditions::LoadConditions, String> {
        // `ICondition.IContext`: the world's feature flags and every registry's
        // entries as this candidate holds them. Tags join later, once this load has bound them.
        let features = || stack.features.iter().map(String::as_str);
        // Admission of a pack entry reads every condition but the registries this builds. It
        // shares the load's verdicts, so a condition that reads no registry runs once per file.
        let base = conditions_for_mods(&self.mods, inventory)?;
        let admission = base.clone().with_features(features());
        // DataStore's fixed content folders omit mod-declared registry folders.
        // Include those entries from this candidate's packs before conditions read them.
        let registry_files = candidate_registry_files(stack)?;
        let registries =
            candidate_registries(root, files.iter().chain(&registry_files), &admission);
        Ok(base.with_features(features()).with_registries(
            registries
                .iter()
                .map(|(registry, ids)| (registry.as_str(), ids.iter().map(String::as_str))),
        ))
    }
}

pub(in crate::game) fn conditions_for_mods(
    sources: &mods::ModsState,
    inventory: &sunrise_mod_package::enabled::EnabledMods,
) -> Result<sunrise_world::conditions::LoadConditions, String> {
    if let Some(candidate) = &sources.load_condition_candidate {
        return Ok(candidate.with_mod_inventory(inventory.server_ids()));
    }
    let table = sources
        .load_conditions
        .get_or_init(|| {
            let mut registrar = mod_api::LoadConditionDeclarations::new(
                sunrise_world::conditions::is_builtin_condition,
            );
            Game::native_register_load_conditions(
                &mod_api::RegistrationContext::new(),
                &mut registrar,
            );
            registrar
                .finish()
                .map(Arc::new)
                .map_err(|error| error.message)
        })
        .clone()?;
    let conditions = sunrise_world::conditions::LoadConditions::new(inventory.server_ids());
    Ok(
        conditions.with_lookup(table.iter().map(|(id, (owner, handler))| {
            let name = id.to_string();
            let enabled = sources.code_owner_enabled(owner);
            let owner = owner.clone();
            let handler = handler.clone();
            (name.clone(), move |args: &serde_json::Value| {
                if !enabled {
                    return Ok(false);
                }
                let mut arguments = args.as_object().cloned().expect("decoded condition object");
                arguments.remove("type");
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    mod_api::__private::contain_refusal(
                        mod_api::content::crash::current_native_mod().unwrap_or("unknown"),
                        || handler.test(&mod_api::serde_json::Value::Object(arguments)),
                    )
                }))
                .map_err(|_| format!("condition {name} (mod {owner}): handler panicked"))
                .and_then(|contained| {
                    contained.map_err(|refusal| {
                        refusal.report();
                        format!("condition {name} (mod {owner}): handler refused: {refusal}")
                    })
                })
                .and_then(|answer| {
                    answer.map_err(|error| {
                        format!("condition {name} (mod {owner}): {}", error.message)
                    })
                })
            })
        })),
    )
}

pub(in crate::game) fn content_root_with_conditions(
    root: &std::path::Path,
    jars: &std::path::Path,
    mods: &[sunrise_client::mod_content::ContentMod],
    cache: &std::path::Path,
    inventory: &sunrise_mod_package::enabled::EnabledMods,
    sources: &mods::ModsState,
) -> sunrise_client::Result<std::path::PathBuf> {
    let conditions = conditions_for_mods(sources, inventory)?;
    sunrise_client::mod_content::resource_root_with_conditions(
        root,
        jars,
        mods,
        cache,
        inventory,
        &conditions,
    )
}

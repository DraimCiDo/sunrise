//! stable-line: content-declarations
//! Content-root condition selection through the owning reload snapshot.
use super::*;






/// Build generated tables using the owning load's registered condition snapshot.
/// Refuse invalid conditions before publishing the generated root.
pub fn build_with_conditions(
    base: &Path,
    jar_store: &Path,
    mods: &[ContentMod],
    out: &Path,
    inventory: &sunrise_mod_package::enabled::EnabledMods,
    conditions: &sunrise_world::conditions::LoadConditions,
) -> Result<Vec<String>> {
    // Two mods owning one namespace would mix their files.
    for (i, a) in mods.iter().enumerate() {
        for b in &mods[..i] {
            let (b, a) = (
                (b.id.as_str(), b.content.namespaces.as_slice()),
                (a.id.as_str(), a.content.namespaces.as_slice()),
            );
            if let Some(e) = sunrise_mod_package::content::clash(b, a) {
                return Err(e.into());
            }
        }
    }
    // The overlay links to the base: a relative base would leave them dangling, and a
    // link through a game folder's own link to the resources would not outlive that folder.
    let base = &real_base(base)?;
    let loaded = load_layers(jar_store, mods)?;
    let plan = Plan::new(base, mods)?;
    // Publish the completed tables as one generation. Passive asset layers
    // retain their original sources instead of being written into this tree.
    let tables_root = out.with_file_name(format!(
        "{}-tables",
        out.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _staging = super::root_lifetime::StagingRoot(tables_root.clone());
    let overlay = Overlay::new(base, &tables_root)?;
    let jobs = table_jobs(base, &overlay)?;
    tables(base, &plan, &overlay, &jobs)?;
    // Mod files, and every tag addition gathered per tag file.
    let mut tags: BTreeMap<String, TagMerge> = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut worldgen: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    let mut additions: BTreeMap<&str, Vec<Addition>> = BTreeMap::new();
    let mut modifier_files = BTreeMap::new();
    let mut modifier_order = 0;
    let mut namespaces: std::collections::BTreeSet<String> = mods
        .iter()
        .flat_map(|m| m.content.namespaces.iter().cloned())
        .collect();
    for group in &loaded {
        let m = group.source;
        let declarations = format!("data/{}/sunrise/", m.id);
        for layer in &group.layers {
            if let Some(jar) = layer.jar {
                for name in layer.data.list("assets/").into_iter().filter(|name| {
                    !crate::packs::merge_stacks(name) && m.content.files.contains(name)
                }) {
                    warnings.push(format!(
                        "mod {}: jar {} also provides {name}; the mod's own file wins",
                        m.id,
                        jar.id()
                    ));
                }
            }
            for file in layer.data.list("data/") {
                if let Some((namespace, _)) = file
                    .strip_prefix("data/")
                    .and_then(|rest| rest.split_once('/'))
                {
                    namespaces.insert(namespace.to_owned());
                }
                let is_tag = file.contains("/tags/") && file.ends_with(".json");
                if !is_tag {
                    if file.starts_with(&declarations) {
                        continue;
                    }
                    let structure = structures::is_modifier(&file);
                    if structure || biomes::is_modifier(&file) {
                        // Registry JSON has one winning source; don't even read a loser.
                        modifier_files.insert(file, (modifier_order, &m.id, layer, structure));
                        modifier_order += 1;
                        continue;
                    }
                    if worldgen_entry(&file).is_none() {
                        continue;
                    }
                }
                let bytes = layer.data.read(&file).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("{}: {file}", layer.assets.id),
                    )
                })?;
                if is_tag {
                    let doc: J =
                        serde_json::from_slice(&bytes).map_err(|e| format!("{file}: {e}"))?;
                    let Some(J::Arr(values)) = doc.get("values") else {
                        return Err(format!("{}: {file}: no values", m.id).into());
                    };
                    let replace = doc.get("replace") == Some(&J::Bool(true));
                    let ns = file.split('/').nth(1).unwrap_or_default();
                    let shared = !m.content.namespaces.iter().any(|n| n == ns);
                    let merge = tags.entry(file.clone()).or_default();
                    warnings.extend(merge.add(&m.id, &file, replace, shared, values));
                    if let Some((_, remove)) = merge.layers.last_mut() {
                        remove.extend(tag_list(&doc, "remove").iter().cloned());
                    }
                } else if let Some((bundle, id)) = worldgen_entry(&file) {
                    let text = String::from_utf8(bytes).map_err(|e| format!("{file}: {e}"))?;
                    let entries = worldgen.entry(bundle).or_default();
                    match Addition::read(bundle, &text).map_err(|e| format!("{file}: {e}"))? {
                        Some(addition) => additions.entry(bundle).or_default().push(addition),
                        None => entries.push((id, text)),
                    }
                }
            }
        }
    }
    let mut winners: Vec<_> = modifier_files.into_iter().collect();
    winners.sort_by_key(|(_, (order, ..))| *order);
    let mut accepted = Vec::new();
    for (file, (_, owner, layer, structure)) in winners {
        let bytes = layer
            .data
            .read(&file)
            .ok_or_else(|| format!("mod {owner}: {file}: unreadable"))?;
        let text = String::from_utf8(bytes).map_err(|e| format!("{file}: {e}"))?;
        let doc = serde_json::from_str(&text).map_err(|e| format!("mod {owner}: {file}: {e}"))?;
        if conditions.allows_file(&doc, &file) {
            accepted.push((file, owner, text, structure));
        }
    }
    conditions.check_errors()?;
    let mut modifiers = Vec::new();
    let mut structure_modifiers = Vec::new();
    for (file, owner, text, structure) in accepted {
        if structure {
            structure_modifiers.extend(structures::Modifier::read(
                owner,
                &file,
                &text,
                &mut warnings,
            )?);
        } else {
            modifiers.extend(biomes::Modifier::read(owner, &file, &text, &mut warnings)?);
        }
    }
    // A tag naming an id nobody registers fails as a whole in vanilla, and
    // carried-over mods name blocks left out or other mods' ids: such
    // entries are dropped instead, so the rest of the tag holds.
    let mod_biomes = worldgen
        .get(BIOMES)
        .into_iter()
        .flatten()
        .map(|(id, _)| id.clone());
    let known = Known::new(base, &plan, &tags, mod_biomes)?;
    for (file, merge) in &mut tags {
        for (values, _) in &mut merge.layers {
            values.retain(|v| known.keeps(file, v, &mut warnings));
        }
    }
    let mut bundles: Vec<(&str, J)> = Vec::new();
    for name in TAG_BUNDLES {
        if base.join(name).exists() {
            bundles.push((name, read_json(&base.join(name))?));
        }
    }
    for (file, merge) in &tags {
        let vanilla = fs::read_to_string(base.join(file))
            .ok()
            .filter(|_| !merge.replaced);
        overlay.write(
            file,
            merge_tag(vanilla.as_deref(), &merge.layers(), file, conditions)?.as_bytes(),
        )?;
        // data/<ns>/tags/block/<path>.json is the block tag <ns>:<path>.
        let Some((ns, path)) = file
            .strip_prefix("data/")
            .and_then(|r| r.split_once("/tags/block/"))
            .and_then(|(ns, p)| Some((ns, p.strip_suffix(".json")?)))
        else {
            continue;
        };
        let id = format!("{ns}:{path}");
        for (name, doc) in &mut bundles {
            let text = match doc.get(&id) {
                Some(J::Str(t)) if !merge.replaced => Some(t.clone()),
                _ => None,
            };
            let merged = merge_tag(text.as_deref(), &merge.layers(), name, conditions)?;
            doc.set(&id, J::Str(merged));
        }
    }
    for (name, doc) in &bundles {
        overlay.write(name, &serde_json::to_vec(doc)?)?;
    }
    // Biome tags, merged into the worldgen loader's bundle as block tags are.
    let biome_tags: Vec<(String, &TagMerge)> = tags
        .iter()
        .filter_map(|(file, merge)| {
            let (ns, path) = file
                .strip_prefix("data/")?
                .split_once("/tags/worldgen/biome/")?;
            Some((format!("{ns}:{}", path.strip_suffix(".json")?), merge))
        })
        .collect();
    let mut biome_tag_doc = None;
    if !biome_tags.is_empty() && base.join(BIOME_TAGS).exists() {
        let mut doc = read_json(&base.join(BIOME_TAGS))?;
        for (id, merge) in biome_tags {
            let text = match doc.get(&id) {
                Some(J::Str(t)) if !merge.replaced => Some(t.clone()),
                _ => None,
            };
            let merged = merge_tag(text.as_deref(), &merge.layers(), BIOME_TAGS, conditions)?;
            doc.set(&id, J::Str(merged));
        }
        overlay.write(BIOME_TAGS, &serde_json::to_vec(&doc)?)?;
        biome_tag_doc = Some(doc);
    }
    // Structures and the pools, sets, processors and features they use, then
    // what mods add to sets, pools and lists, by priority, then in mod and
    // file order.
    let mut placed_doc = None;
    let mut biome_doc = None;
    // A mod's biomes follow vanilla's, by namespace then path.
    if let Some(entries) = worldgen.get_mut(BIOMES) {
        entries.sort_by(|a, b| a.0.split_once(':').cmp(&b.0.split_once(':')));
    }
    for (bundle, entries) in &worldgen {
        if !base.join(bundle).exists() {
            continue;
        }
        let mut doc = read_json(&base.join(bundle))?;
        for (id, text) in entries {
            doc.set(id, J::Str(text.clone()));
        }
        if let Some(list) = additions.get_mut(bundle) {
            apply_additions(&mut doc, list).map_err(|e| format!("{bundle}: {e}"))?;
        }
        overlay.write(bundle, &serde_json::to_vec(&doc)?)?;
        if *bundle == PLACED_FEATURES {
            placed_doc = Some(doc);
        } else if *bundle == BIOMES {
            biome_doc = Some(doc);
        }
    }
    if !modifiers.is_empty() || (!mods.is_empty() && base.join(BIOMES).exists()) {
        place_in_biomes(
            base,
            &overlay,
            &modifiers,
            inventory.server_ids().map(str::to_owned).collect(),
            biome_tag_doc,
            placed_doc,
            biome_doc,
            &tags,
            &mut warnings,
        )?;
    }
    if !structure_modifiers.is_empty() {
        structures::apply(base, &overlay, &structure_modifiers, &tags, &mut warnings)?;
    }
    // The commands suggest ids from `command-registries.json`: add the
    // structures, pools, sets and features the mods bring.
    let registries = out_path(&overlay, "command-registries.json");
    if let Ok(mut doc) = read_json(&registries) {
        // Registry names hold `/`, so each key is looked up by itself.
        let mut add = |keys: &[&str], new: &[J]| {
            let found = keys.iter().try_fold(&mut doc, |j, k| j.get_mut(k));
            if let Some(J::Arr(ids)) = found {
                for id in new {
                    if !ids.contains(id) {
                        ids.push(id.clone());
                    }
                }
            }
        };
        for (bundle, registry) in COMMAND_REGISTRIES {
            let new: Vec<J> = worldgen
                .get(bundle)
                .into_iter()
                .flatten()
                .map(|(id, _)| J::Str(id.clone()))
                .collect();
            add(&["by_registry", registry, "ids"], &new);
        }
        overlay.write("command-registries.json", &serde_json::to_vec(&doc)?)?;
    }
    // A block without its own blockstate looks like its template, so every
    // state still has a model.
    for m in mods {
        for block in &m.content.blocks {
            let (ns, path) = block.id.split_once(':').unwrap_or((&m.id, &block.id));
            let own = format!("assets/{ns}/blockstates/{path}.json");
            if m.content.files.contains(&own) {
                continue;
            }
            let template = block.template.trim_start_matches("minecraft:");
            let vanilla = base.join(format!("assets/minecraft/blockstates/{template}.json"));
            overlay.write(&own, &fs::read(&vanilla)?)?;
        }
    }
    // Vanilla's namespace isn't a content mod's, even when a mod ships files
    // under it (`data/minecraft/tags/...`): every loader reads it already, and
    // listed again it read each vanilla recipe twice, once as `minecraft:<name>`.
    namespaces.remove("minecraft");
    overlay.write(MARKER, &serde_json::to_vec(&namespaces)?)?;
    let requirements =
        collect_jar_requirements(mods.iter().map(|m| (m.id.as_str(), m.jars.as_slice())))?;
    overlay.write(JAR_INPUTS, &serde_json::to_vec(&requirements)?)?;
    overlay.write(
        sunrise_mod_package::enabled::MARKER,
        &serde_json::to_vec(inventory)?,
    )?;
    let by_item = |value: fn(&NewItem) -> Option<&str>| {
        let items = plan.items.iter();
        items
            .filter_map(|i| Some((i.rename.to.as_str(), value(i)?)))
            .collect::<BTreeMap<_, _>>()
    };
    use sunrise_items::items::{MOD_CRAFT_REMAINDERS, MOD_ITEM_BLOCKS, MOD_ITEM_TEMPLATES};
    for (name, map) in [
        (
            MOD_CRAFT_REMAINDERS,
            by_item(|i| i.crafting_remainder.as_deref()),
        ),
        (MOD_ITEM_TEMPLATES, by_item(|i| i.template.as_deref())),
        (MOD_ITEM_BLOCKS, by_item(|i| i.block.as_deref())),
    ] {
        if !map.is_empty() {
            overlay.write(name, &serde_json::to_vec(&map)?)?;
        }
    }
    let arm_poses: BTreeMap<&str, sunrise_mod_package::content::ArmPose> = plan
        .items
        .iter()
        .filter_map(|i| Some((i.rename.to.as_str(), i.arm_pose?)))
        .collect();
    if !arm_poses.is_empty() {
        overlay.write(
            sunrise_items::items::MOD_ITEM_ARM_POSES,
            &serde_json::to_vec(&arm_poses)?,
        )?;
    }
    overlay.write(
        BLOCK_ENTITY_TYPES,
        &serde_json::to_vec(&plan.block_entity_factories)?,
    )?;
    // clients must agree on the block states and the items', mob effects',
    // entity types', block entity types' and menu types' network ids.
    if !plan.blocks.is_empty()
        || !plan.items.is_empty()
        || !plan.effects.is_empty()
        || !plan.fluids.is_empty()
        || !plan.entity_types.is_empty()
        || !plan.block_entity_types.is_empty()
        || !plan.menu_types.is_empty()
    {
        overlay.write(crate::content_identity::MARKER, b"1")?;
    }
    overlay.write(WARNINGS, warnings.join("\n").as_bytes())?;
    let generated = serde_json::to_vec(
        &*overlay
            .written
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )?;
    overlay.write(GENERATED_FILES, &generated)?;
    let _ = fs::remove_dir_all(out);
    fs::rename(&tables_root, out)?;
    Ok(warnings)
}

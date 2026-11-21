//! stable-line: content-declarations
//! The references a content mod's files make to each other, which `Content::read` leaves for the
//! game to meet at load: a blockstate's models, a model's parent and textures, an item
//! definition's model, the ids a recipe, a tag or a loot table names in the mod's own namespaces,
//! and what a block needs to be playable (a blockstate, a loot table, a name).
//!
//! The tool checks only fields that the game's formats treat as references: models through the
//! game's `CuboidModel` decoder, blockstates and item definitions through `sunrise-assets`' decoders
//! (mod-defined types are left alone), and the other files through the shapes the recipe, tag and
//! loot loaders walk (a recipe's ingredients and result, a tag's `values`, a loot entry's `name`
//! and a state condition's `block`). Components, custom data and fields of unknown recipe types
//! are not scanned. The generic JSON reader accepts a byte-order mark and comments. A
//! file the load conditions (`neoforge:conditions`) may disable, a tag entry that is not
//! `required`, and a removal are not read: they cannot break a mod that does not have what they
//! name.
//!
//! Ids outside the mod's namespaces (vanilla's blocks, items, templates and loot functions) are
//! not checked: their tables live in the game's prepared resources, which no package tool reads.
//!
//! Severity is one rule. An **error** is a finding where the tool sees everything that decides
//! the answer and the game would certainly drop or draw the thing broken: an id of the mod's own
//! namespaces that nothing in the mod declares, in a mod with no code, no `[[jar]]` layer and no declared dependency (whose jar can supply files; the
//! namespace fence keeps every other mod out of them), a file the mod lacks when nothing else
//! could supply it, a file that is not JSON at all. A **warning** is a finding where another
//! source could supply the thing (a `[[jar]]` layer or a dependency's jar, an atlas source that renames a sprite or
//! walks a directory, a block whose template draws without a block model) or a legal mistake
//! (a block with no loot table or name); it says "could not find ... in this mod; if X supplies
//! it, ignore this". What the tool cannot judge at all is dropped, not warned: the ids a mod's
//! code declares, what the load conditions decide, a payload only a mod reads.
use super::{Content, FileReader, RenderShape};
use crate::manifest::{Manifest, Side};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use sunrise_assets::{
    blockstate::{BlockStateFile, VariantSet},
    item_model::ClientItem,
    model::{CuboidModel, Loader, TextureSlot},
};

/// What [`check_references`] found, each a sentence that names the file.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Findings {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// The most declared ids a message lists.
const LISTED: usize = 8;

struct Model {
    file: String,
    definition: CuboidModel,
}

#[derive(Clone, Copy)]
enum Asset {
    Model,
    Parent,
    Texture { inline_override: bool },
}

/// Checks the references among `content`'s files; `Err` only when a file cannot be read.
/// A native mod also declares ids in code, so the ids it declares in files
/// are not taken for all of them; a mod with `jar` layers may find assets and data there.
pub fn check_references(content: &Content, manifest: &Manifest) -> Result<Findings, String> {
    let atlas = |name: &String| {
        let mut parts = name.split('/');
        parts.next() == Some("assets") && parts.nth(1) == Some("atlases") && name.ends_with(".json")
    };
    let mut check = References {
        content,
        manifest,
        reader: content.reader()?,
        code: manifest.native.is_some(),
        jar: !manifest.jar.is_empty() || !manifest.dependencies.is_empty(),
        atlas: content.files.iter().any(atlas),
        recipe_filters: content.files.iter().any(|name| {
            name.strip_prefix("data/")
                .and_then(|data| data.split_once('/'))
                .is_some_and(|(_, path)| {
                    path.starts_with("recipe_filter/") && path.ends_with(".json")
                })
        }),
        blocks: content.blocks.iter().map(|b| b.id.clone()).collect(),
        items: content
            .items
            .iter()
            .map(|i| i.id.clone())
            .chain(
                content
                    .blocks
                    .iter()
                    .filter(|b| b.item)
                    .map(|b| b.id.clone()),
            )
            .collect(),
        // A world fluid's flowing form is a fluid of its own (its block and bucket are in
        // `content.blocks` and `content.items`).
        fluids: content
            .fluids
            .iter()
            .flat_map(|f| [f.id.clone(), f.flowing_id()])
            .collect(),
        entity_types: content.entity_types.iter().map(|t| t.id.clone()).collect(),
        models: BTreeMap::new(),
        findings: Findings::default(),
    };
    for name in &content.files {
        let mut parts = name.splitn(3, '/');
        let (top, namespace, rest) = (parts.next(), parts.next(), parts.next());
        let (Some(top), Some(namespace), Some(rest)) = (top, namespace, rest) else {
            continue;
        };
        let Some(path) = rest.strip_suffix(".json") else {
            continue;
        };
        let owned = content.namespaces.iter().any(|n| n == namespace);
        match (top, path.split_once('/')) {
            ("assets", Some(("blockstates", _))) if owned => check.blockstate(name)?,
            ("assets", Some(("models", model))) if owned => {
                check.read_model(name, namespace, model)?
            }
            ("assets", Some(("items", _))) if owned => check.item_definition(name)?,
            ("data", Some(("recipe", _))) if owned => check.recipe(name)?,
            ("data", Some(("loot_table", _))) if owned => check.loot_table(name)?,
            ("data", Some(("tags", tag))) => check.tag(name, tag)?,
            _ => {}
        }
    }
    check.check_models();
    check.declarations()?;
    check.side();
    // Four slots of one model naming one missing texture are one finding.
    for list in [&mut check.findings.errors, &mut check.findings.warnings] {
        let mut seen = BTreeSet::new();
        list.retain(|message| seen.insert(message.clone()));
    }
    Ok(check.findings)
}

struct References<'a> {
    content: &'a Content,
    manifest: &'a Manifest,
    reader: FileReader<'a>,
    code: bool,
    /// A `[[jar]]` layer or a declared dependency's jar may supply files of the mod's namespaces.
    jar: bool,
    /// The mod has atlas sources, which can rename a sprite or take a whole directory.
    atlas: bool,
    /// A filter file may remove recipes or replace a referenced input or result before decoding.
    recipe_filters: bool,
    blocks: BTreeSet<String>,
    items: BTreeSet<String>,
    fluids: BTreeSet<String>,
    entity_types: BTreeSet<String>,
    /// Every model file of the mod's namespaces, by id.
    models: BTreeMap<String, Model>,
    findings: Findings,
}

impl References<'_> {
    fn has(&self, file: &str) -> bool {
        self.content
            .files
            .binary_search_by(|f| f.as_str().cmp(file))
            .is_ok()
    }

    /// The path of `id` when it is in one of the mod's namespaces.
    fn own<'i>(&self, id: &'i str) -> Option<(&'i str, &'i str)> {
        let (namespace, path) = id.split_once(':')?;
        self.content
            .namespaces
            .iter()
            .any(|n| n == namespace)
            .then_some((namespace, path))
    }

    /// What else could supply an asset the mod's files lack.
    fn supplier(&self, texture: bool) -> Option<String> {
        let mut others = Vec::new();
        if self.jar {
            others.push("a [[jar]] layer or a dependency's jar");
        }
        if texture && self.atlas {
            others.push("an atlas source (it renames a sprite or reads a directory)");
        }
        (!others.is_empty()).then(|| others.join(" or "))
    }

    /// `id` as a file of this mod, `assets/<ns>/<folder>/<path><extension>`: an error when the
    /// file is missing and nothing else could supply it, a warning when something could.
    fn missing(&mut self, name: &str, field: &str, id: &str, kind: Asset) {
        let Some((namespace, path)) = self.own(id) else {
            return;
        };
        let (folder, extension, what) = match kind {
            Asset::Model => ("models", ".json", "model"),
            Asset::Parent => ("models", ".json", "parent model"),
            Asset::Texture { .. } => ("textures", ".png", "texture"),
        };
        let file = format!("assets/{namespace}/{folder}/{path}{extension}");
        if self.has(&file) {
            return;
        }
        let near = near_file(self, folder, extension, id);
        let supplier = self.supplier(folder == "textures");
        let reason = match (kind, supplier) {
            (
                Asset::Texture {
                    inline_override: true,
                },
                _,
            ) => Some("an inline composite child may override the slot".to_owned()),
            (_, Some(other)) => Some(format!("{other} may supply it")),
            (_, None) => None,
        };
        match reason {
            None => self.findings.errors.push(format!(
                "{name}: the {what} {id} at {field} is not a file of this mod (expected {file}). Add the file or fix the id{near}."
            )),
            Some(reason) => self.findings.warnings.push(format!(
                "{name}: could not find the {what} {id} at {field} in this mod (expected {file}); {reason}. If not, add the file or fix the id{near}."
            )),
        }
    }

    /// Follow parent chains from both model files and inline children. An inline override only
    /// affects that child, which may itself be hidden; it cannot prove the parent sprite is unused.
    fn check_models(&mut self) {
        let mut overridden = BTreeSet::new();
        let mut inline_overridden = BTreeSet::new();
        let mut references = Vec::new();
        for (id, model) in &self.models {
            let mut pending = vec![(&model.definition, String::new())];
            while let Some((child, field)) = pending.pop() {
                if let Some(Loader::Composite { children, .. }) = &child.loader {
                    pending.extend(children.iter().map(|(key, child)| {
                        (child.as_ref(), format!("{field}children[{key:?}]."))
                    }));
                }
                let Some(parent) = child.parent.as_deref() else {
                    continue;
                };
                references.push((
                    model.file.clone(),
                    format!("{field}parent"),
                    parent.to_owned(),
                    Asset::Parent,
                ));
                let (inline, overrides) = if field.is_empty() {
                    (false, &mut overridden)
                } else {
                    (true, &mut inline_overridden)
                };
                let mut at = Some(parent);
                for _ in 0..64 {
                    let Some(ancestor) = at else { break };
                    if !inline && ancestor == id {
                        break;
                    }
                    let Some(parent) = self.models.get(ancestor) else {
                        break;
                    };
                    for slot in child.textures.keys() {
                        if parent.definition.textures.contains_key(slot) {
                            overrides.insert((ancestor, slot.as_str()));
                        }
                    }
                    at = parent.definition.parent.as_deref();
                }
            }
        }
        for (id, model) in &self.models {
            // One diagnostic per missing sprite, preferring a slot with no override.
            let mut by_sprite = BTreeMap::new();
            for (slot, texture) in &model.definition.textures {
                let key = (id.as_str(), slot.as_str());
                if let TextureSlot::Material(material) = texture
                    && !overridden.contains(&key)
                {
                    let inline_override = inline_overridden.contains(&key);
                    let entry = by_sprite
                        .entry(&material.sprite)
                        .or_insert((slot, inline_override));
                    if !inline_override {
                        *entry = (slot, false);
                    }
                }
            }
            references.extend(
                by_sprite
                    .into_iter()
                    .map(|(sprite, (slot, inline_override))| {
                        (
                            model.file.clone(),
                            format!("textures[{slot:?}]"),
                            sprite.clone(),
                            Asset::Texture { inline_override },
                        )
                    }),
            );
        }
        for (file, field, id, kind) in references {
            self.missing(&file, &field, &id, kind);
        }
    }

    fn tag_reference(&mut self, name: &str, field: &str, id: &str, tag: &str, registry: &str) {
        let Some((namespace, path)) = self.own(tag) else {
            return;
        };
        let file = format!("data/{namespace}/tags/{registry}/{path}.json");
        if self.has(&file) {
            return;
        }
        let other = match (self.jar, self.code) {
            (true, true) => Some("a [[jar]] layer, a dependency's jar or the mod's code"),
            (true, false) => Some("a [[jar]] layer or a dependency's jar"),
            (false, true) => Some("the mod's code"),
            (false, false) => None,
        };
        match other {
            None => self.findings.errors.push(format!(
                "{name}: the tag {id} at {field} is not a tag file of this mod (expected {file}), so the tag does not load. Add the file or fix the id."
            )),
            Some(other) => self.findings.warnings.push(format!(
                "{name}: could not find the tag {id} at {field} in this mod (expected {file}); if {other} supplies it, ignore this. Otherwise add the file or fix the id."
            )),
        }
    }

    /// `id` of a vanilla-shaped registry the mod declares into, when it is in the mod's namespace
    /// and the mod does not declare it: an error unless a jar may declare it or the reference
    /// may not be used. A mod with code is not asked: its code declares ids no file shows.
    fn id_in_registry(
        &mut self,
        name: &str,
        id: &str,
        registry: &str,
        what: &str,
        uncertainty: Option<&str>,
    ) {
        if self.code || self.own(id).is_none() {
            return;
        }
        let known: Vec<&str> = match registry {
            "block" => &self.blocks,
            "item" => &self.items,
            "fluid" => &self.fluids,
            "entity_type" => &self.entity_types,
            _ => return,
        }
        .iter()
        .map(String::as_str)
        .collect();
        if known.contains(&id) {
            return;
        }
        let article = if registry == "item" || registry == "entity_type" {
            "an"
        } else {
            "a"
        };
        let kind = registry.replace('_', " ");
        let near = nearest(id, &known)
            .map(|n| format!(" Did you mean {n}?"))
            .unwrap_or_default();
        let listed = self.listed(&known);
        let reason = uncertainty.or(self
            .jar
            .then_some("a [[jar]] layer or a dependency's jar may supply it"));
        if let Some(reason) = reason {
            self.findings.warnings.push(format!(
                "{name}: could not find {what} {id} among the {kind}s this mod declares ({listed}); {reason}.{near} Otherwise fix the id."
            ));
        } else {
            self.findings.errors.push(format!(
                "{name}: {what} {id} is not {article} {kind} this mod declares ({listed}).{near} Fix the id."
            ));
        }
    }

    fn listed(&self, known: &[&str]) -> String {
        if known.is_empty() {
            return "none declared".into();
        }
        let mut names: Vec<&str> = known
            .iter()
            .map(|id| {
                id.strip_prefix(self.manifest.id.as_str())
                    .and_then(|rest| rest.strip_prefix(':'))
                    .unwrap_or(id)
            })
            .collect();
        names.sort_unstable();
        let more = names.len().saturating_sub(LISTED);
        names.truncate(LISTED);
        let mut text = format!("declared: {}", names.join(", "));
        if more > 0 {
            text.push_str(&format!(", and {more} more"));
        }
        text
    }

    /// What each block and item declared in a file needs: a blockstate, an item definition, a
    /// loot table, a name.
    fn declarations(&mut self) -> Result<(), String> {
        let mut names = Vec::new();
        let content = self.content;
        for block in &content.blocks {
            let Some((namespace, path)) = self.own(&block.id) else {
                continue;
            };
            if !self.has(&format!("data/{namespace}/sunrise/block/{path}.json")) {
                continue;
            }
            let file = format!("data/{namespace}/sunrise/block/{path}.json");
            let state = format!("assets/{namespace}/blockstates/{path}.json");
            // Only a block drawn from a model needs a blockstate. One that declares no render
            // shape takes its template's, which the tool cannot see: a chest or a sign is drawn
            // by its block entity.
            let shape = block.properties.render_shape;
            if shape != Some(RenderShape::Invisible)
                && shape != Some(RenderShape::EntityblockAnimated)
                && !self.has(&state)
            {
                match (shape, self.supplier(false)) {
                    (Some(_), None) => self.findings.errors.push(format!(
                        "{file}: the block {} has no model: expected {state}, which names its models. Add it, or declare render_shape INVISIBLE.",
                        block.id
                    )),
                    (shape, other) => {
                        let mut maybe = Vec::new();
                        if shape.is_none() {
                            maybe.push("its template draws it without a block model (a chest, a sign)".to_owned());
                        }
                        maybe.extend(other.map(|o| format!("{o} supplies it")));
                        self.findings.warnings.push(format!(
                            "{file}: could not find a blockstate for the block {} in this mod (expected {state}); if {}, ignore this. Otherwise add it, or declare render_shape INVISIBLE.",
                            block.id,
                            maybe.join(" or ")
                        ));
                    }
                }
            }
            if block.item {
                self.item_definition_exists(&file, &block.id, namespace, path);
            }
            let loot = format!("data/{namespace}/loot_table/blocks/{path}.json");
            if !self.jar && !self.has(&loot) {
                self.findings.warnings.push(format!(
                    "{file}: no local loot table found for the block {} (expected {loot}). Add one if this block needs its own table.",
                    block.id
                ));
            }
            names.push((
                format!("block.{namespace}.{}", path.replace('/', ".")),
                file,
            ));
        }
        for item in &content.items {
            let Some((namespace, path)) = self.own(&item.id) else {
                continue;
            };
            let file = format!("data/{namespace}/sunrise/item/{path}.json");
            if !self.has(&file) {
                continue;
            }
            self.item_definition_exists(&file, &item.id, namespace, path);
            names.push((format!("item.{namespace}.{}", path.replace('/', ".")), file));
        }
        self.names(names)
    }

    fn item_definition_exists(&mut self, file: &str, id: &str, namespace: &str, path: &str) {
        let definition = format!("assets/{namespace}/items/{path}.json");
        if !self.jar && !self.has(&definition) {
            self.findings.warnings.push(format!(
                "{file}: the item {id} has no item definition, so it is drawn as the missing model. Add {definition}."
            ));
        }
    }

    /// Registry content on a client-only package: joining a server compares the mods, and the
    /// package's side defaults to `client` when the manifest has no `side` line.
    fn side(&mut self) {
        let content = &self.content;
        let registry = !(content.blocks.is_empty()
            && content.items.is_empty()
            && content.effects.is_empty()
            && content.fluids.is_empty()
            && content.entity_types.is_empty()
            && content.creative_tabs.is_empty());
        if registry && self.manifest.side == Side::Client {
            self.findings.warnings.push(
                "sunrise-mod.toml: side is client (the default when the line is left out), but this mod adds blocks, items or other registry content that a server and its players must have too; set side = \"both\" or joining a server fails."
                    .into(),
            );
        }
    }
}

/// `", did you mean <id>"` for a missing file, from the files of the same folder.
fn near_file(check: &References<'_>, folder: &str, extension: &str, id: &str) -> String {
    let Some((namespace, _)) = check.own(id) else {
        return String::new();
    };
    let prefix = format!("assets/{namespace}/{folder}/");
    let known: Vec<String> = check
        .content
        .files
        .iter()
        .filter_map(|f| {
            let path = f.strip_prefix(&prefix)?.strip_suffix(extension)?;
            Some(format!("{namespace}:{path}"))
        })
        .collect();
    let known: Vec<&str> = known.iter().map(String::as_str).collect();
    nearest(id, &known)
        .map(|n| format!(" (did you mean {n}?)"))
        .unwrap_or_default()
}

/// The id among `known` within two edits of `id`, the closest first.
fn nearest<'k>(id: &str, known: &[&'k str]) -> Option<&'k str> {
    known
        .iter()
        .map(|k| (edits(id, k), *k))
        .filter(|(d, _)| *d <= 2)
        .min()
        .map(|(_, k)| k)
}

/// The number of single-character changes from `a` to `b` (Levenshtein).
fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(ca != *cb));
            diagonal = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
#[path = "references_tests.rs"]
mod tests;

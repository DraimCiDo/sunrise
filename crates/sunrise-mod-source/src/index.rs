//! The Rust items a set of crates' signatures name, found as rustc finds them: through each
//! module's items, `use`s and glob imports, into the crates `sunrise-mod-api` builds on.
use std::{collections::HashMap, fs, path::Path};
use syn::{Item, UseTree, Visibility};

/// One module: its crate, its path below the crate root, and its items.
pub struct Module {
    pub krate: &'static str,
    pub path: Vec<String>,
    pub items: Vec<Item>,
}

/// A named item a signature refers to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ItemId {
    pub module: usize,
    pub item: usize,
}

/// Where a name leads.
pub enum Found {
    Item(ItemId),
    /// A path outside the indexed crates (`std::…`, `serde_json::…`), as written.
    External(Vec<String>),
}

/// The indexed crates' modules, parsed as they are first reached.
pub struct Index<'a> {
    root: &'a Path,
    /// The crates it indexes: crate name and source directory below `root`.
    crates: &'static [(&'static str, &'static str)],
    /// Sources included into an inline module with `include!`, by file name.
    included: &'a [(&'a str, &'a str)],
    pub modules: Vec<Module>,
    by_path: HashMap<(&'static str, Vec<String>), usize>,
    /// How deep `resolve` is in following imports.
    depth: usize,
    /// Every source file read, for the build script to track.
    pub read: std::cell::RefCell<Vec<std::path::PathBuf>>,
    /// Rewrites a source file's text before it is parsed: the game's generated module lists.
    expand: fn(&Path, &str) -> Result<String, String>,
    /// Crates added for one consumer beside its crates ([`Index::add_crate`]).
    extra: Vec<(&'static str, &'static str)>,
    /// A crate's root file where it is not `lib.rs` (a library's `[lib] path`).
    root_files: HashMap<&'static str, String>,
}

/// The file a module's `#[path = "..."]` names.
fn path_attribute(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        syn::Meta::NameValue(meta) if meta.path.is_ident("path") => match &meta.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(file),
                ..
            }) => Some(file.value()),
            _ => None,
        },
        _ => None,
    })
}

/// The directory below the crate root that holds `path`'s last module, with its slash.
fn parent_dir(path: &[String]) -> String {
    path.split_last().map_or(String::new(), |(_, dir)| {
        dir.iter().map(|d| format!("{d}/")).collect()
    })
}

impl<'a> Index<'a> {
    pub fn new(
        root: &'a Path,
        included: &'a [(&'a str, &'a str)],
        crates: &'static [(&'static str, &'static str)],
    ) -> Self {
        Self {
            root,
            crates,
            expand: |_, source| Ok(source.to_owned()),
            included,
            modules: Vec::new(),
            by_path: HashMap::new(),
            depth: 0,
            read: Default::default(),
            extra: Vec::new(),
            root_files: HashMap::new(),
        }
    }

    /// Names the file `krate`'s root module lives in, below its source directory.
    pub fn set_root_file(&mut self, krate: &'static str, file: &str) {
        self.root_files.insert(krate, file.to_owned());
    }

    /// Reads each source file through `expand` before parsing it.
    pub fn expanding(mut self, expand: fn(&Path, &str) -> Result<String, String>) -> Self {
        self.expand = expand;
        self
    }

    /// Indexes one more crate, for this index only: the surface walk reads the facade crate
    /// (`sunrise_mod_api`), which no generated output resolves through.
    pub fn add_crate(&mut self, name: &'static str, dir: &'static str) {
        if self.crate_dir(name).is_none() {
            self.extra.push((name, dir));
        }
    }

    fn crate_dir(&self, name: &str) -> Option<(&'static str, &'static str)> {
        self.crates
            .iter()
            .chain(&self.extra)
            .find(|(krate, _)| *krate == name)
            .copied()
    }

    pub fn item(&self, id: ItemId) -> &Item {
        &self.modules[id.module].items[id.item]
    }

    /// The module at `path` in `krate`, parsed on first use.
    pub fn module(&mut self, krate: &str, path: &[String]) -> Result<usize, String> {
        let krate = self
            .crate_dir(krate)
            .map(|(name, _)| name)
            .ok_or_else(|| format!("{krate} is not a crate the mod API builds on"))?;
        if let Some(&id) = self.by_path.get(&(krate, path.to_vec())) {
            return Ok(id);
        }
        let items = match path.split_last() {
            None => {
                let file = self.root_files.get(krate).map_or("lib.rs", String::as_str);
                self.parse(krate, file)?
            }
            Some((last, parent)) => {
                let parent = self.module(krate, parent)?;
                let declared = self.modules[parent]
                    .items
                    .iter()
                    .find_map(|item| match item {
                        Item::Mod(item) if item.ident == last.as_str() => {
                            Some((item.content.clone(), path_attribute(&item.attrs)))
                        }
                        _ => None,
                    });
                match declared {
                    Some((Some((_, items)), _)) => self.expand_includes(items)?,
                    // A generated `#[path]` is absolute.
                    Some((None, Some(file))) if Path::new(&file).is_absolute() => {
                        self.parse(krate, &file)?
                    }
                    // `#[path]` is relative to the declaring file's directory: `parent.rs`
                    // sits beside it, `parent/mod.rs` below.
                    Some((None, Some(file))) => {
                        let beside = parent_dir(&path[..path.len() - 1]);
                        let below = parent_dir(path);
                        self.parse(krate, &format!("{beside}{file}"))
                            .or_else(|_| self.parse(krate, &format!("{below}{file}")))?
                    }
                    _ => {
                        let dir = path.join("/");
                        self.parse(krate, &format!("{dir}.rs"))
                            .or_else(|_| self.parse(krate, &format!("{dir}/mod.rs")))?
                    }
                }
            }
        };
        let id = self.modules.len();
        self.modules.push(Module {
            krate,
            path: path.to_vec(),
            items,
        });
        self.by_path.insert((krate, path.to_vec()), id);
        Ok(id)
    }

    fn parse(&self, krate: &str, file: &str) -> Result<Vec<Item>, String> {
        let dir = self.crate_dir(krate).expect("a known crate").1;
        let path = self.root.join(dir).join(file);
        let source = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.read.borrow_mut().push(path.clone());
        let source = (self.expand)(self.root, &source)?;
        let file = syn::parse_file(&source).map_err(|e| format!("{}: {e}", path.display()))?;
        self.expand_includes(file.items)
    }

    /// `items` with each `include!` of a generated file replaced by that file's items.
    fn expand_includes(&self, items: Vec<Item>) -> Result<Vec<Item>, String> {
        let mut out = Vec::new();
        for item in items {
            match &item {
                Item::Macro(mac) if mac.mac.path.is_ident("include") => {
                    let tokens = mac.mac.tokens.to_string();
                    // The wire impls are generated from what this reads, so it never reads them.
                    if tokens.contains("wire_") {
                        continue;
                    }
                    let (name, source) = self
                        .included
                        .iter()
                        .find(|(name, _)| tokens.contains(name))
                        .ok_or_else(|| format!("include!({tokens}) is not a generated file"))?;
                    let file = syn::parse_file(source).map_err(|e| format!("{name}: {e}"))?;
                    out.extend(file.items);
                }
                _ => out.push(item),
            }
        }
        Ok(out)
    }

    /// What `name` means in `module`: an item defined there, a `use` of it, or a glob import
    /// that provides it.
    pub fn resolve(&mut self, module: usize, name: &str) -> Result<Found, String> {
        self.depth += 1;
        if self.depth > 64 {
            self.depth = 0;
            let m = &self.modules[module];
            return Err(format!(
                "{name} in {}::{} imports itself",
                m.krate,
                m.path.join("::")
            ));
        }
        let found = self.lookup(module, name, &mut vec![module], &mut Vec::new());
        self.depth = self.depth.saturating_sub(1);
        found?.ok_or_else(|| {
            let m = &self.modules[module];
            format!("{name} not found in {}::{}", m.krate, m.path.join("::"))
        })
    }

    /// `name` as bound in `module`, seen from every module in `holders`: the module the lookup
    /// started in, then each module whose glob led here. A glob imports only what is visible
    /// from the module holding it, and a module's own definition or named import of `name`
    /// shadows its globs even where that binding is not visible.
    fn lookup(
        &mut self,
        module: usize,
        name: &str,
        holders: &mut Vec<usize>,
        seen: &mut Vec<usize>,
    ) -> Result<Option<Found>, String> {
        if seen.contains(&module) {
            return Ok(None);
        }
        seen.push(module);
        let items = &self.modules[module].items;
        let reachable = |vis: &Visibility| holders.iter().all(|&h| self.visible(module, h, vis));
        let mut shadowed = false;
        for (item, definition) in items.iter().enumerate() {
            if let Some(vis) = definition_visibility(definition, name) {
                if reachable(vis) {
                    return Ok(Some(Found::Item(ItemId { module, item })));
                }
                shadowed = true;
            }
        }
        let mut named = None;
        let mut globs = Vec::new();
        for item in items {
            if let Item::Use(item) = item {
                let open = reachable(&item.vis);
                uses(
                    &item.tree,
                    &mut Vec::new(),
                    &mut |path, alias| match alias {
                        Some(alias) if alias == name && named.is_none() => {
                            named = Some((path, open));
                        }
                        None if open => globs.push(path),
                        _ => {}
                    },
                );
            }
        }
        match named {
            Some((path, true)) => return self.resolve_path(module, &path).map(Some),
            Some((_, false)) => return Ok(None),
            None if shadowed => return Ok(None),
            None => {}
        }
        for glob in globs {
            if let Some((krate, path)) = self.absolute(module, &glob)
                && let Ok(target) = self.module(krate, &path)
            {
                holders.push(module);
                let found = self.lookup(target, name, holders, seen);
                holders.pop();
                if let Some(found) = found? {
                    return Ok(Some(found));
                }
            }
        }
        Ok(None)
    }

    /// A glob does not import a sibling's private definitions or local `use` bindings.
    fn visible(&self, module: usize, from: usize, vis: &Visibility) -> bool {
        if module == from {
            return true;
        }
        let module_path = &self.modules[module];
        let from = &self.modules[from];
        match vis {
            Visibility::Public(_) => true,
            _ if module_path.krate != from.krate => false,
            Visibility::Inherited => from.path.starts_with(&module_path.path),
            Visibility::Restricted(vis) => {
                let path: Vec<_> = vis
                    .path
                    .segments
                    .iter()
                    .map(|seg| seg.ident.to_string())
                    .collect();
                self.absolute(module, &path).is_some_and(|(krate, scope)| {
                    from.krate == krate && from.path.starts_with(&scope)
                })
            }
        }
    }

    /// What the path `path`, written in `module`, names.
    pub fn resolve_path(&mut self, module: usize, path: &[String]) -> Result<Found, String> {
        if path.len() == 1 {
            return self.resolve(module, &path[0]);
        }
        let (last, parent) = path.split_last().unwrap();
        match self.absolute(module, parent) {
            Some((krate, parent)) => {
                // A path through a type (`Enum::Variant`) is not a module.
                let target = self.module(krate, &parent)?;
                self.resolve(target, last)
            }
            None => {
                // The first segment may be a name `use`d into `module` (a module alias).
                if let Ok(Found::External(prefix)) = self.resolve(module, &path[0])
                    && prefix.len() > 1
                {
                    let mut full = prefix;
                    full.extend(path[1..].iter().cloned());
                    return Ok(Found::External(full));
                }
                Ok(Found::External(path.to_vec()))
            }
        }
    }

    /// `path`, written in `module`, as a module of an indexed crate, if it is one.
    pub fn absolute(&self, module: usize, path: &[String]) -> Option<(&'static str, Vec<String>)> {
        let m = &self.modules[module];
        let (first, rest) = path.split_first()?;
        let mut base = match first.as_str() {
            "crate" => (m.krate, Vec::new()),
            "self" => (m.krate, m.path.clone()),
            "super" => (m.krate, m.path[..m.path.len().checked_sub(1)?].to_vec()),
            name if m
                .items
                .iter()
                .any(|item| matches!(item, Item::Mod(item) if item.ident == name)) =>
            {
                let mut path = m.path.clone();
                path.push(name.to_string());
                (m.krate, path)
            }
            name => (self.crate_dir(name)?.0, Vec::new()),
        };
        for segment in rest {
            if segment == "super" {
                base.1.pop()?;
            } else {
                base.1.push(segment.clone());
            }
        }
        Some(base)
    }
}

/// The visibility of an indexed definition named `name`, if this item defines it.
fn definition_visibility<'a>(item: &'a Item, name: &str) -> Option<&'a Visibility> {
    let (ident, vis) = match item {
        Item::Struct(item) => (&item.ident, &item.vis),
        Item::Enum(item) => (&item.ident, &item.vis),
        Item::Type(item) => (&item.ident, &item.vis),
        Item::Trait(item) => (&item.ident, &item.vis),
        Item::Union(item) => (&item.ident, &item.vis),
        Item::Const(item) => (&item.ident, &item.vis),
        _ => return None,
    };
    (ident == name).then_some(vis)
}

/// Each path `tree` imports, with the name it binds (`None` for a glob).
pub fn uses(
    tree: &UseTree,
    prefix: &mut Vec<String>,
    f: &mut impl FnMut(Vec<String>, Option<String>),
) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            uses(&path.tree, prefix, f);
            prefix.pop();
        }
        UseTree::Name(name) => {
            let mut path = prefix.clone();
            if name.ident != "self" {
                path.push(name.ident.to_string());
            }
            let alias = path.last().cloned();
            f(path, alias);
        }
        UseTree::Rename(rename) => {
            let mut path = prefix.clone();
            if rename.ident != "self" {
                path.push(rename.ident.to_string());
            }
            f(path, Some(rename.rename.to_string()));
        }
        UseTree::Glob(_) => f(prefix.clone(), None),
        UseTree::Group(group) => {
            for tree in &group.items {
                uses(tree, prefix, f);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `name` resolves to in `module` of a one-crate index built from `lib`.
    fn resolves(lib: &str, module: &[&str], name: &str) -> Option<Vec<String>> {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("k/src")).unwrap();
        fs::write(dir.path().join("k/src/lib.rs"), lib).unwrap();
        let mut index = Index::new(dir.path(), &[], &[("k", "k/src")]);
        let path: Vec<String> = module.iter().map(|m| (*m).to_owned()).collect();
        let module = index.module("k", &path).unwrap();
        match index.resolve(module, name) {
            Ok(Found::Item(id)) => Some(index.modules[id.module].path.clone()),
            Ok(Found::External(path)) => panic!("{name} left the crate: {path:?}"),
            Err(_) => None,
        }
    }

    #[test]
    fn a_glob_of_a_glob_imports_only_what_every_glob_holder_can_see() {
        let lib = "
            pub mod z {
                pub mod y { pub(super) struct Hidden; pub struct Shown; }
                pub mod w { use crate::*; }
            }
            pub use z::y::*;
        ";
        assert_eq!(
            resolves(lib, &["z", "w"], "Shown"),
            Some(vec!["z".into(), "y".into()])
        );
        assert_eq!(resolves(lib, &["z", "w"], "Hidden"), None);
    }

    #[test]
    fn a_definition_hidden_from_the_importer_still_shadows_its_modules_globs() {
        let lib = "
            pub mod a { struct Thing; pub use crate::b::*; }
            pub mod b { pub struct Thing; pub struct Other; }
            pub mod c { use crate::a::*; }
        ";
        assert_eq!(resolves(lib, &["c"], "Other"), Some(vec!["b".into()]));
        assert_eq!(resolves(lib, &["c"], "Thing"), None);
    }
}

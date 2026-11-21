/// Content files per mod, bytes per file and bytes per mod.
pub const MAX_FILES: usize = 65_536;

pub const MAX_FILE: u64 = 16 << 20;

pub const MAX_TOTAL: u64 = 512 << 20;

/// Checks a content path: `assets/` or `data/`, then `/`-separated parts of
/// `[a-z0-9_.-]`, none of them `.` or `..` (vanilla's resource-location
/// characters, so the path means the same on every OS).
pub fn check_path(name: &str) -> Result<(), String> {
    let plain_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c);
    let mut parts = name.split('/');
    let top = parts.next();
    // assets/<namespace>/<file> at least.
    let named_ok = matches!(top, Some("assets" | "data"))
        && parts.clone().count() >= 2
        && parts
            .clone()
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if named_ok && parts.clone().all(|part| part.chars().all(plain_char)) {
        return Ok(());
    }
    let mut error = format!(
        "{name:?} is not a plain assets/ or data/ path: after assets/ or data/, every part is made of a-z, 0-9, _, . and - (vanilla's resource-location characters, so the path means the same on every OS), and none is . or .."
    );
    if named_ok {
        let fixed: Vec<String> = parts
            .map(|part| {
                part.to_ascii_lowercase()
                    .chars()
                    .map(|c| if plain_char(c) { c } else { '_' })
                    .collect()
            })
            .collect();
        error.push_str(&format!(
            "; rename it {:?}",
            format!("{}/{}", top.unwrap_or(""), fixed.join("/"))
        ));
    }
    Err(error)
}

#[derive(Debug, Clone)]
enum Source {
    Dir(PathBuf),
    Zip(PathBuf),
    /// No files: content a native mod declares in code.
    Code,
}

/// A mod's content: its files and declarations.
#[derive(Debug)]
pub struct Content {
    source: Source,
    /// Every content file (`assets/...`, `data/...`), sorted.
    pub files: Vec<String>,
    pub blocks: Vec<Block>,
    pub items: Vec<Item>,
    pub effects: Vec<MobEffect>,
    /// Its creative inventory tabs, by id.
    pub creative_tabs: Vec<CreativeTab>,
    /// How its fluids look, by id.
    pub fluids: Vec<FluidLook>,
    /// What its liquid blocks turn into beside another fluid or block, in file-name order.
    pub fluid_interactions: Vec<FluidInteraction>,
    pub entity_types: Vec<EntityType>,
    /// Block entity types declared by native code, not content files.
    pub block_entity_types: Vec<BlockEntityType>,
    pub menu_types: Vec<MenuType>,
    /// The namespaces it owns: the mod id, then the manifest's `namespaces`.
    pub namespaces: Vec<String>,
}

/// Reads a mod's content files ([`Content::reader`]).
pub enum FileReader<'a> {
    Dir(&'a Path),
    Zip(Box<zip::ZipArchive<std::fs::File>>),
}

impl FileReader<'_> {
    /// One content file's bytes.
    pub fn read(&mut self, name: &str) -> Result<Vec<u8>, String> {
        check_path(name)?;
        let reader: Box<dyn Read + '_> = match self {
            Self::Dir(root) => {
                Box::new(std::fs::File::open(root.join(name)).map_err(|e| format!("{name}: {e}"))?)
            }
            Self::Zip(zip) => Box::new(zip.by_name(name).map_err(|e| format!("{name}: {e}"))?),
        };
        let mut out = Vec::new();
        reader
            .take(MAX_FILE + 1)
            .read_to_end(&mut out)
            .map_err(|e| format!("{name}: {e}"))?;
        if out.len() as u64 > MAX_FILE {
            return Err(format!("{name} is over {} MiB", MAX_FILE >> 20));
        }
        Ok(out)
    }
    /// A content file's path on disk, checked as [`read`](Self::read) checks it (the name,
    /// the size), for a caller that copies it without reading it; `None` in an archive.
    pub fn path(&self, name: &str) -> Result<Option<std::path::PathBuf>, String> {
        check_path(name)?;
        let root = match self {
            Self::Dir(root) => root,
            Self::Zip(_) => return Ok(None),
        };
        let path = root.join(name);
        let len = std::fs::metadata(&path)
            .map_err(|e| format!("{name}: {e}"))?
            .len();
        if len > MAX_FILE {
            return Err(format!("{name} is over {} MiB", MAX_FILE >> 20));
        }
        Ok(Some(path))
    }
}

/// Why two content mods cannot be on together: a namespace both own (one's
/// id or listed namespace is the other's), which would mix their files.
/// `a` and `b` are (mod id, owned namespaces).
pub fn clash(a: (&str, &[String]), b: (&str, &[String])) -> Option<String> {
    let shared = a.1.iter().find(|ns| b.1.contains(ns))?;
    Some(format!(
        "mods {} and {} both use the namespace {shared}; turn one of them off",
        a.0, b.0
    ))
}

impl Content {
    /// The content of the mod at `path` (a `.smod` or a mod folder).
    pub fn read(path: &Path, manifest: &Manifest) -> Result<Content, String> {
        Self::read_declaring(path, manifest, &[])
    }

    /// [`Content::read`] for a mod whose native code also declares items (`native_items`):
    /// a file's `crafting_remainder` may name one of those `declared` ids, which are not in the
    /// files.
    pub fn read_declaring(
        path: &Path,
        manifest: &Manifest,
        declared: &[String],
    ) -> Result<Content, String> {
        let (source, sized) = if path.is_dir() {
            (Source::Dir(path.to_owned()), folder_files(path)?)
        } else {
            let mut zip = open_zip(path)?;
            let mut out = Vec::new();
            for i in 0..zip.len() {
                let entry = zip.by_index_raw(i).map_err(|e| e.to_string())?;
                let name = entry.name().map_err(|e| e.to_string())?.into_owned();
                if (name.starts_with("assets/") || name.starts_with("data/")) && !entry.is_dir() {
                    out.push((name.to_owned(), entry.size()));
                }
            }
            (Source::Zip(path.to_owned()), out)
        };
        let mut files = Vec::with_capacity(sized.len());
        let mut total = 0u64;
        for (name, size) in sized {
            check_path(&name)?;
            if size > MAX_FILE {
                return Err(format!("{name} is over {} MiB", MAX_FILE >> 20));
            }
            total += size;
            files.push(name);
        }
        if files.len() > MAX_FILES {
            return Err(format!("more than {MAX_FILES} content files"));
        }
        if total > MAX_TOTAL {
            return Err(format!("content is over {} MiB", MAX_TOTAL >> 20));
        }
        files.sort();
        files.dedup();
        let mut content = Content {
            source,
            files,
            blocks: Vec::new(),
            items: Vec::new(),
            effects: Vec::new(),
            creative_tabs: Vec::new(),
            fluids: Vec::new(),
            fluid_interactions: Vec::new(),
            entity_types: Vec::new(),
            block_entity_types: Vec::new(),
            menu_types: Vec::new(),
            namespaces: manifest.owned_namespaces().map(str::to_owned).collect(),
        };
        content.check(manifest, declared)?;
        Ok(content)
    }

    /// Content with no files, for native declarations in `namespace`
    /// ([`Content::declare`]).
    pub fn declared(namespace: &str) -> Content {
        Content {
            source: Source::Code,
            files: Vec::new(),
            blocks: Vec::new(),
            items: Vec::new(),
            effects: Vec::new(),
            creative_tabs: Vec::new(),
            fluids: Vec::new(),
            fluid_interactions: Vec::new(),
            entity_types: Vec::new(),
            block_entity_types: Vec::new(),
            menu_types: Vec::new(),
            namespaces: vec![namespace.to_owned()],
        }
    }

    /// Adds blocks and items a native mod declares in code (`native_blocks`,
    /// `native_items`), checked as declaration files are: ids in any of this
    /// mod's owned namespaces (its id and manifest `namespaces`) and declared once
    /// with what it already has, a vanilla template, a crafting remainder
    /// that is vanilla or this mod's. All or nothing: on an error nothing is
    /// added.
    pub fn declare(
        &mut self,
        blocks: &[Block],
        items: &[Item],
    ) -> Result<(), Vec<(DeclarationKind, DeclarationError)>> {
        let in_own = |id: &str| {
            if namespaced_id(id)
                && id
                    .split_once(':')
                    .is_some_and(|(ns, _)| self.namespaces.iter().any(|owned| owned == ns))
            {
                Ok(())
            } else {
                Err(format!(
                    "not an id in owned namespaces {:?}",
                    self.namespaces
                ))
            }
        };
        self.check_declaration_batch(
            blocks
                .iter()
                .map(|b| (b, in_own(&b.id).and_then(|()| check_block(b)))),
            items
                .iter()
                .map(|i| (i, in_own(&i.id).and_then(|()| check_item(i)))),
        )?;
        self.blocks.extend_from_slice(blocks);
        self.items.extend_from_slice(items);
        Ok(())
    }

    /// Joins declarations whose fields and submitting namespaces were checked at registration.
    pub fn declare_validated(
        &mut self,
        blocks: &[LocalDeclaration<Block>],
        items: &[LocalDeclaration<Item>],
    ) -> Result<(), Vec<(DeclarationKind, DeclarationError)>> {
        self.check_declaration_batch(
            blocks.iter().map(|b| (b.get(), Ok(()))),
            items.iter().map(|i| (i.get(), Ok(()))),
        )?;
        self.blocks.extend(blocks.iter().map(|b| b.get().clone()));
        self.items.extend(items.iter().map(|i| i.get().clone()));
        Ok(())
    }

    fn check_declaration_batch<'a>(
        &self,
        blocks: impl IntoIterator<Item = (&'a Block, Result<(), String>)>,
        items: impl IntoIterator<Item = (&'a Item, Result<(), String>)>,
    ) -> Result<(), Vec<(DeclarationKind, DeclarationError)>> {
        let mut ids: BTreeSet<&str> = self
            .blocks
            .iter()
            .map(|b| b.id.as_str())
            .chain(self.items.iter().map(|i| i.id.as_str()))
            .collect();
        let mut item_ids: BTreeSet<&str> = self
            .blocks
            .iter()
            .filter(|b| b.item)
            .map(|b| b.id.as_str())
            .chain(self.items.iter().map(|i| i.id.as_str()))
            .collect();
        let mut errors = Vec::new();
        let mut refuse = |kind, id: &str, reason| {
            errors.push((
                kind,
                DeclarationError::Rejected {
                    id: id.to_owned(),
                    reason,
                },
            ));
        };
        let mut all_blocks: Vec<&Block> = self.blocks.iter().collect();
        for (block, fields) in blocks {
            let id = block.id.as_str();
            let result = fields.and_then(|()| {
                if !ids.insert(id) {
                    return Err("declared twice".into());
                }
                if block.item {
                    item_ids.insert(id);
                }
                Ok(())
            });
            match result {
                Ok(()) => all_blocks.push(block),
                Err(reason) => refuse(DeclarationKind::Block, id, reason),
            }
        }
        sign_wall_refusals(&all_blocks)
            .for_each(|(id, reason)| refuse(DeclarationKind::Block, id, reason));
        let mut valid_items = Vec::new();
        for (item, fields) in items {
            let id = item.id.as_str();
            let result = fields.and_then(|()| {
                if !item_ids.insert(id) {
                    return Err("declared twice, or is also a block's item".into());
                }
                Ok(())
            });
            match result {
                Ok(()) => valid_items.push(item),
                Err(reason) => refuse(DeclarationKind::Item, id, reason),
            }
        }
        for item in valid_items {
            if let Some(rest) = &item.crafting_remainder
                && (rest == "minecraft:air"
                    || !vanilla_id(rest) && !item_ids.contains(rest.as_str()))
            {
                refuse(
                    DeclarationKind::Item,
                    &item.id,
                    format!(
                        "crafting_remainder {rest:?} is not a vanilla item (minecraft:...) or one of this mod's"
                    ),
                );
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(())
    }

    /// Where the files are read from: the mod's folder or its `.smod`.
    pub fn source(&self) -> &Path {
        match &self.source {
            Source::Dir(path) => path,
            Source::Zip(path) => path,
            Source::Code => Path::new(""),
        }
    }

    /// No files: a mod with code only.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The refusal of a `side = "server"` package whose files declare registry content
    /// (`data/<owned namespace>/sunrise/`), naming the mod and the first such file: every client
    /// must have that content. Assets and data vanilla syncs itself (recipes, loot, tags) are
    /// allowed. None for a package of any other side.
    pub fn server_only_refusal(&self, manifest: &Manifest) -> Option<String> {
        if manifest.side != crate::manifest::Side::Server {
            return None;
        }
        let name = self.files.iter().find(|name| {
            let mut parts = name.splitn(3, '/');
            let (top, namespace, rest) = (parts.next(), parts.next(), parts.next());
            top == Some("data")
                && rest.is_some_and(|rest| rest.starts_with("sunrise/"))
                && manifest
                    .owned_namespaces()
                    .any(|owned| Some(owned) == namespace)
        })?;
        Some(format!(
            "{}: a side = \"server\" mod carries {name}, which every client must have; \
             declare side = \"both\"",
            manifest.id
        ))
    }

    /// One content file's bytes.
    pub fn file(&self, name: &str) -> Result<Vec<u8>, String> {
        self.reader()?.read(name)
    }

    /// A reader of its content files. A packaged mod's archive is opened
    /// once for all of them (opening it reads its whole index).
    pub fn reader(&self) -> Result<FileReader<'_>, String> {
        reader_of(&self.source)
    }
}

fn reader_of(source: &Source) -> Result<FileReader<'_>, String> {
    Ok(match source {
        Source::Dir(root) => FileReader::Dir(root),
        Source::Zip(path) => FileReader::Zip(Box::new(open_zip(path)?)),
        Source::Code => FileReader::Dir(Path::new("")),
    })
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    zip::ZipArchive::new(file).map_err(|e| e.to_string())
}

/// Files under `dir`, relative to `root` with `/` separators. Symbolic links
/// are not followed: a mod folder holds its own files.
pub(crate) fn folder_files(root: &Path) -> Result<Vec<(String, u64)>, String> {
    let mut out = Vec::new();
    for top in ["assets", "data"] {
        walk(root, &root.join(top), &mut out)?;
    }
    Ok(out)
}

/// The files under a package folder's `runtime/`, as `(name below it, bytes)`. Symbolic links and
/// other non-files are refused, not followed.
pub(crate) fn runtime_folder_files(root: &Path) -> Result<Vec<(String, u64)>, String> {
    let mut out = Vec::new();
    walk(root, &root.join(crate::manifest::RUNTIME_DIR), &mut out)?;
    let prefix = format!("{}/", crate::manifest::RUNTIME_DIR);
    for (name, _) in &mut out {
        *name = name.strip_prefix(&prefix).unwrap_or(name).to_owned();
    }
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) -> Result<(), String> {
    match std::fs::symlink_metadata(dir) {
        Ok(m) if !m.is_dir() => {
            return Err(format!(
                "{}: unsupported file type in a content mod",
                dir.display()
            ));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    }
    let read = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in read {
        let path = entry.map_err(|e| e.to_string())?.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            walk(root, &path, out)?;
        } else if meta.is_file() {
            let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
            let parts: Option<Vec<&str>> = rel.iter().map(|p| p.to_str()).collect();
            let name = parts.ok_or_else(|| format!("{} is not UTF-8", rel.display()))?;
            out.push((name.join("/"), meta.len()));
        } else {
            return Err(format!(
                "{}: unsupported file type in a content mod",
                path.display()
            ));
        }
        if out.len() > MAX_FILES {
            return Err(format!("more than {MAX_FILES} content files"));
        }
    }
    Ok(())
}

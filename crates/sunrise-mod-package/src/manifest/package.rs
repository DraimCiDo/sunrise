//! Read and write packages containing a mod manifest.

use super::{MANIFEST, MAX_RUNTIME_FILE, Manifest, RUNTIME_DIR, RuntimeData, RuntimeTable, Side};
use mod_surface::uses::USES_FILE;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

/// The largest uses table ([`USES_FILE`]) a package may carry: what the surface crate reads.
pub const MAX_USES: u64 = mod_surface::uses::MAX_USES_BYTES as u64;

/// A mod's saved settings, shared by native mods and modpack overrides.
pub const STORAGE_DIR: &str = ".sunrise-mod-data";

/// Maximum manifest bytes accepted by package readers and developer tools.
pub const MAX_MANIFEST: u64 = 32 << 20;

/// The most source files (`Cargo.toml`, `Cargo.lock`, `build.rs`, `src/**`, the root notices) a
/// package selects: as many as its content files, so a port laid out one file per class fits.
pub const MAX_SOURCE_FILES: usize = crate::content::MAX_FILES;

/// The most bytes those source files take, unpacked.
pub const MAX_SOURCE_BYTES: u64 = 64 << 20;

/// Folders of a mod's project that are never part of its package: Git's, Cargo's build output
/// and the pinned SDK the mod tool fetches into `.sunrise/`.
pub const NOT_PACKAGED: [&str; 3] = [".git", "target", ".sunrise"];

impl Manifest {
    /// The namespaces the mod's content owns: its id, then `namespaces`.
    pub fn owned_namespaces(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.id.as_str()).chain(self.namespaces.iter().map(String::as_str))
    }

    /// The refusal of a `side = "server"` mod whose code declares registry content (one of
    /// `declared`, each its registry kind and id, in a namespace it owns), naming the mod, the
    /// kind and the id: every client must have that content.
    pub fn server_only_declaration<'a>(
        &self,
        mut declared: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> Option<String> {
        if self.side != Side::Server {
            return None;
        }
        let (kind, id) = declared.find(|(_, id)| {
            let ns = id.split_once(':').map_or("", |(ns, _)| ns);
            self.owned_namespaces().any(|owned| owned == ns)
        })?;
        Some(format!(
            "{}: a side = \"server\" mod declares the {kind} {id}, which every client must have; \
             declare side = \"both\"",
            self.id
        ))
    }
}

/// A source file that belongs to the flat Cargo project, not its resource packs.
pub fn source_file(name: &str) -> bool {
    if name.split('/').any(|part| NOT_PACKAGED.contains(&part)) {
        return false;
    }
    name == "Cargo.toml"
        || name == "Cargo.lock"
        || name == "build.rs"
        || (name.starts_with("src/") && (name.ends_with(".rs") || name.ends_with(".toml")))
        || (!name.contains('/')
            && ["README", "LICENSE", "LICENCE", "COPYING", "NOTICE"]
                .iter()
                .any(|prefix| name.starts_with(prefix)))
}

pub(crate) fn plain_path(name: &str) -> bool {
    !name.starts_with('/')
        && !name.contains(['\\', ':'])
        && !name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

/// A mod as found on disk.
pub struct Package {
    pub manifest: Manifest,
    pub icon: Option<crate::icon::Icon>,
}

impl Package {
    /// The kind of this validated package, independent of its install folder.
    pub fn is_native(&self) -> bool {
        self.manifest.native.is_some()
    }

    /// Read a flat `.smod` or project folder, without executing native code.
    pub fn read(path: &Path) -> Result<Self, String> {
        let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        let mut icon = None;
        let mut has_cargo = false;
        let mut has_source = false;
        let toml = if metadata.is_dir() {
            for old in [
                "sunrise-native.json",
                "horizon-mixin.toml",
                "native",
                "rust",
            ] {
                if std::fs::symlink_metadata(path.join(old)).is_ok() {
                    return Err(format!("{old}: unsupported package path"));
                }
            }
            if std::fs::symlink_metadata(path.join("Cargo.toml")).is_ok() {
                check_folder_file(path, "Cargo.toml")?;
                has_cargo = true;
            }
            if let Ok(source) = std::fs::symlink_metadata(path.join("src")) {
                if !source.is_dir() {
                    return Err("src: unsupported package path".into());
                }
                has_source = true;
            }
            let icon_path = path.join("icon.png");
            match std::fs::symlink_metadata(&icon_path) {
                Ok(metadata) => {
                    check_folder_file(path, "icon.png")?;
                    if metadata.len() > crate::icon::MAX_ICON as u64 {
                        return Err("icon.png exceeds 256 KiB".into());
                    }
                    let mut bytes = Vec::new();
                    std::fs::File::open(&icon_path)
                        .map_err(|e| format!("icon.png: {e}"))?
                        .take(crate::icon::MAX_ICON as u64 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|e| format!("icon.png: {e}"))?;
                    icon = Some(crate::icon::Icon::parse(bytes)?);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("icon.png: {e}")),
            }
            check_folder_file(path, MANIFEST)?;
            for (name, _) in crate::content::folder_files(path)? {
                check_folder_file(path, &name)?;
            }
            let mut text = String::new();
            std::fs::File::open(path.join(MANIFEST))
                .map_err(|e| format!("{MANIFEST}: {e}"))?
                .take(MAX_MANIFEST + 1)
                .read_to_string(&mut text)
                .map_err(|e| format!("{MANIFEST}: {e}"))?;
            if text.len() as u64 > MAX_MANIFEST {
                return Err(format!("{MANIFEST}: over {} MiB", MAX_MANIFEST >> 20));
            }
            text
        } else {
            if !metadata.is_file() {
                return Err(format!(
                    "{}: unsupported file type in a content mod",
                    path.display()
                ));
            }
            let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
            return Self::read_zip(&mut zip);
        };
        let package = Self::from_parts(&toml, icon, has_cargo, has_source)?;
        package.check_runtime_layout(&crate::content::runtime_folder_files(path)?)?;
        Ok(package)
    }

    /// Read and validate an archive, including all selected source and resource entries.
    pub fn read_zip<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>) -> Result<Self, String> {
        let mut icon = None;
        let mut has_cargo = false;
        let mut has_source = false;
        let mut entries = Vec::new();
        let mut uses = false;
        let mut runtime = Vec::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().map_err(|e| e.to_string())?.into_owned();
            let mode = entry.unix_mode().unwrap_or_default();
            entries.push((name.clone(), entry.size(), entry.is_dir()));
            let first = name.split('/').next().unwrap_or("");
            if matches!(
                first,
                "sunrise-native.json" | "horizon-mixin.toml" | "native" | "rust"
            ) {
                return Err(format!("{name}: unsupported package path"));
            }
            has_source |= first == "src";
            if entry.is_dir() && matches!(mode & 0o170000, 0 | 0o040000) {
                continue;
            }
            has_cargo |= name == "Cargo.toml";
            if let Some(below) = name
                .strip_prefix(RUNTIME_DIR)
                .and_then(|n| n.strip_prefix('/'))
            {
                // Any file type, but a file: its bytes are checked against `[runtime]` below.
                if !matches!(mode & 0o170000, 0 | 0o100000) {
                    return Err(format!("{name}: unsupported file type"));
                }
                runtime.push((below.to_owned(), entry.size()));
                continue;
            }
            if name == USES_FILE && matches!(mode & 0o170000, 0 | 0o100000) {
                if uses {
                    return Err(format!("duplicate {USES_FILE}"));
                }
                if entry.size() > MAX_USES {
                    return Err(format!("{USES_FILE} exceeds {} MiB", MAX_USES >> 20));
                }
                uses = true;
                continue;
            }
            check_file(&name, mode, || Ok(&mut entry)).map_err(|e| e.to_string())?;
            if name == "icon.png" {
                if icon.is_some() {
                    return Err("duplicate icon.png".into());
                }
                if entry.size() > crate::icon::MAX_ICON as u64 {
                    return Err("icon.png exceeds 256 KiB".into());
                }
                let mut bytes = Vec::new();
                drop(entry);
                zip.by_index(i)
                    .map_err(|e| e.to_string())?
                    .take(crate::icon::MAX_ICON as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| format!("icon.png: {e}"))?;
                icon = Some(crate::icon::Icon::parse(bytes)?);
            }
        }
        let mut toml = String::new();
        zip.by_name(MANIFEST)
            .map_err(|e| format!("{MANIFEST}: {e}"))?
            .take(MAX_MANIFEST + 1)
            .read_to_string(&mut toml)
            .map_err(|e| format!("{MANIFEST}: {e}"))?;
        if toml.len() as u64 > MAX_MANIFEST {
            return Err(format!("{MANIFEST}: over {} MiB", MAX_MANIFEST >> 20));
        }
        let package = Self::from_parts(&toml, icon, has_cargo, has_source)?;
        if uses && !package.is_native() {
            return Err(format!("a content mod has no code, so no {USES_FILE}"));
        }
        package.check_runtime_layout(&runtime)?;
        if package.is_native() {
            let mut names = std::collections::HashSet::new();
            let mut source_count = 0usize;
            let mut source_bytes = 0u64;
            let mut resource_count = 0usize;
            let mut resource_bytes = 0u64;
            for (name, size, directory) in entries {
                let path = if directory {
                    name.trim_end_matches('/')
                } else {
                    &name
                };
                if !plain_path(path) || !names.insert(name.clone()) {
                    return Err(format!("invalid or duplicate package path: {name}"));
                }
                if !directory {
                    if name != MANIFEST
                        && name != USES_FILE
                        && name != "icon.png"
                        && !name.starts_with("runtime/")
                        && !name.starts_with("assets/")
                        && !name.starts_with("data/")
                        && !source_file(&name)
                    {
                        return Err(format!("unexpected package path: {name}"));
                    }
                    if source_file(&name) {
                        source_count += 1;
                        source_bytes = source_bytes
                            .checked_add(size)
                            .ok_or("native package too large")?;
                    } else if name.starts_with("assets/") || name.starts_with("data/") {
                        crate::content::check_path(&name)?;
                        crate::content::check_namespace(&name, &package.manifest)?;
                        if size > crate::content::MAX_FILE {
                            return Err(format!("{name} exceeds the content file size limit"));
                        }
                        resource_count += 1;
                        resource_bytes = resource_bytes
                            .checked_add(size)
                            .ok_or("content too large")?;
                    }
                }
            }
            if source_count > MAX_SOURCE_FILES || source_bytes > MAX_SOURCE_BYTES {
                return Err("native package exceeds its entry or size limit".into());
            }
            if resource_count > crate::content::MAX_FILES
                || resource_bytes > crate::content::MAX_TOTAL
            {
                return Err("content exceeds its entry or size limit".into());
            }
        }
        Ok(package)
    }

    /// Refuse a `runtime/` folder that is not the one `[runtime]` lists: an extra, a missing or an
    /// oversized file, each named. `found` is each file below `runtime/` and its size.
    fn check_runtime_layout(&self, found: &[(String, u64)]) -> Result<(), String> {
        if !found.is_empty() && !self.is_native() {
            return Err("a content mod has no code, so no runtime/ folder".into());
        }
        RuntimeTable::check_layout(self.manifest.runtime.as_ref(), found)
    }

    /// The package's runtime files, read from `path` (a folder or a `.smod`) and each checked
    /// against its `[runtime]` SHA-512: the one place their bytes are hashed. A mismatch is
    /// refused by name. `None` for a package with no `[runtime]`.
    pub fn read_runtime(&self, path: &Path) -> Result<Option<RuntimeData>, String> {
        let Some(table) = &self.manifest.runtime else {
            return Ok(None);
        };
        let mut files = std::collections::BTreeMap::new();
        let folder = path.is_dir();
        let mut zip = if folder {
            None
        } else {
            let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            Some(zip::ZipArchive::new(file).map_err(|e| e.to_string())?)
        };
        for (name, _) in table.files() {
            let entry = format!("{RUNTIME_DIR}/{name}");
            let mut bytes = Vec::new();
            // At most one byte past the cap, so an oversized file is refused unread.
            let read = if folder {
                std::fs::File::open(path.join(&entry))
                    .map_err(|e| format!("{entry}: {e}"))?
                    .take(MAX_RUNTIME_FILE + 1)
                    .read_to_end(&mut bytes)
            } else {
                zip.as_mut()
                    .expect("an archive")
                    .by_name(&entry)
                    .map_err(|e| format!("{entry}: {e}"))?
                    .take(MAX_RUNTIME_FILE + 1)
                    .read_to_end(&mut bytes)
            };
            read.map_err(|e| format!("{entry}: {e}"))?;
            table.check(name, &bytes)?;
            files.insert(name.to_owned(), std::sync::Arc::from(bytes));
        }
        Ok(Some(RuntimeData::new(
            files,
            table.digest(),
            folder.then(|| path.to_owned()),
        )))
    }

    fn from_parts(
        text: &str,
        icon: Option<crate::icon::Icon>,
        has_cargo: bool,
        has_source: bool,
    ) -> Result<Self, String> {
        let manifest = Manifest::parse(text)?;
        if has_cargo != manifest.native.is_some() || has_source != has_cargo {
            return Err("[native] and Rust source must be declared together".into());
        }
        Ok(Package { manifest, icon })
    }
    /// Select and validate a flat project's source and resource files deterministically.
    pub fn directory_files(
        &self,
        root: &Path,
    ) -> Result<std::collections::BTreeMap<String, Vec<u8>>, String> {
        let content = crate::Content::read(root, &self.manifest)?;
        let mut files = std::collections::BTreeMap::new();
        let mut reader = content.reader()?;
        for name in &content.files {
            files.insert(name.clone(), reader.read(name)?);
        }
        if let Some(icon) = &self.icon {
            files.insert("icon.png".to_string(), icon.bytes().to_vec());
        }
        if let Some(runtime) = self.read_runtime(root)? {
            for name in runtime.names() {
                files.insert(
                    format!("{RUNTIME_DIR}/{name}"),
                    runtime.file(name).expect("a listed file").to_vec(),
                );
            }
        }
        if self.is_native() {
            fn walk(
                root: &Path,
                dir: &Path,
                files: &mut std::collections::BTreeMap<String, Vec<u8>>,
                source_bytes: &mut u64,
                source_count: &mut usize,
            ) -> Result<(), String> {
                for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    let path = entry.path();
                    let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
                    let name = relative
                        .to_str()
                        .ok_or("non-UTF-8 package path")?
                        .replace('\\', "/");
                    let kind = entry.file_type().map_err(|e| e.to_string())?;
                    if kind.is_dir() {
                        if entry
                            .file_name()
                            .to_str()
                            .is_some_and(|name| NOT_PACKAGED.contains(&name))
                            || matches!(name.as_str(), "assets" | "data" | RUNTIME_DIR)
                        {
                            continue;
                        }
                        walk(root, &path, files, source_bytes, source_count)?;
                    } else if source_file(&name) {
                        check_folder_file(root, &name)?;
                        let size = entry.metadata().map_err(|e| e.to_string())?.len();
                        *source_bytes = source_bytes
                            .checked_add(size)
                            .ok_or("native package too large")?;
                        *source_count += 1;
                        if *source_bytes > MAX_SOURCE_BYTES || *source_count > MAX_SOURCE_FILES {
                            return Err("native package exceeds its entry or size limit".into());
                        }
                        files.insert(name, std::fs::read(path).map_err(|e| e.to_string())?);
                    } else if name.ends_with(".rs") || (name.ends_with(".toml") && name != MANIFEST)
                    {
                        return Err(format!("unexpected package path: {name}"));
                    }
                }
                Ok(())
            }
            let (mut source_bytes, mut source_count) = (0, 0);
            walk(root, root, &mut files, &mut source_bytes, &mut source_count)?
        }
        files.insert(
            MANIFEST.to_owned(),
            std::fs::read(root.join(MANIFEST)).map_err(|e| e.to_string())?,
        );
        Ok(files)
    }

    /// Pack a flat project, selecting its source and resource files deterministically.
    pub fn pack_directory(root: &Path) -> Result<Vec<u8>, String> {
        Self::pack_directory_with(root, None, None)
    }

    /// The bytes of entry `name` in the `.smod` at `path`, refused past `most` bytes; `None` when
    /// it has no such entry.
    pub fn entry(path: &Path, name: &str, most: u64) -> Result<Option<Vec<u8>>, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", path.display()))?;
        let entry = match zip.by_name(name) {
            Ok(entry) => entry,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let mut bytes = Vec::new();
        entry
            .take(most + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{name}: {e}"))?;
        if bytes.len() as u64 > most {
            return Err(format!("{name}: over {most} bytes"));
        }
        Ok(Some(bytes))
    }

    /// [`Package::pack_directory`], carrying `uses` as the package's [`USES_FILE`] table when given.
    pub fn pack_directory_with(
        root: &Path,
        uses: Option<&[u8]>,
        manifest: Option<String>,
    ) -> Result<Vec<u8>, String> {
        let package = Self::read(root)?;
        let mut files = package.directory_files(root)?;
        if let Some(uses) = uses {
            files.insert(USES_FILE.to_owned(), uses.to_vec());
        }
        let selected = files.remove(MANIFEST).expect("selected manifest");
        let text = match manifest {
            Some(text) => text,
            None => String::from_utf8(selected).map_err(|e| e.to_string())?,
        };
        let files: Vec<_> = files
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect();
        Self::write_files(&text, &files)
    }

    /// The `.smod` bytes for `manifest_text` and selected project files.
    pub fn write_files(manifest_text: &str, files: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
        use std::io::Write;
        let manifest = Manifest::parse(manifest_text)?;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let err = |e: zip::result::ZipError| e.to_string();
        let mut put = |name: &str, bytes: &[u8]| -> Result<(), String> {
            if minisign_secret_key(bytes) {
                return Err(format!("{name}: a Minisign private key cannot be packaged"));
            }
            zip.start_file(name, options).map_err(err)?;
            zip.write_all(bytes).map_err(|e| e.to_string())
        };
        put(MANIFEST, manifest_text.as_bytes())?;
        let code = manifest.native.is_some();
        for (name, bytes) in files {
            let source = code && source_file(name) && plain_path(name);
            if let Some(below) = name.strip_prefix("runtime/") {
                let table = manifest
                    .runtime
                    .as_ref()
                    .ok_or(format!("{name}: not listed in [runtime]"))?;
                table.check(below, bytes)?;
                put(name, bytes)?;
                continue;
            }
            if *name == USES_FILE && code {
                if bytes.len() as u64 > MAX_USES {
                    return Err(format!("{USES_FILE} exceeds {} MiB", MAX_USES >> 20));
                }
                put(name, bytes)?;
                continue;
            }
            if *name != "icon.png" && !source {
                crate::content::check_path(name)?;
            }
            check_file(name, 0, || Ok(*bytes)).map_err(|e| e.to_string())?;
            put(name, bytes)?;
        }
        let bytes = zip.finish().map_err(err)?.into_inner();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).map_err(err)?;
        Self::read_zip(&mut archive)?;
        Ok(bytes)
    }
}

/// Recognize a Minisign secret-key record by its framing, without decrypting or copying the secret.
/// The 158-byte payload starts with Ed25519, a scrypt or empty KDF, and a BLAKE2b checksum tag.
fn minisign_secret_key(bytes: &[u8]) -> bool {
    use base64::{Engine, engine::general_purpose::STANDARD};
    const KEY_BYTES: usize = 158;
    let header = |bytes: &[u8]| matches!(bytes, b"EdScB2" | b"Ed\0\0B2");
    if bytes.len() >= KEY_BYTES && header(&bytes[..6]) {
        return true;
    }
    // The text format has an arbitrary comment line, then the base64 payload. The comment
    // is not authentication: a renamed file or a different comment still carries the key.
    let Some(line) = bytes.splitn(3, |&byte| byte == b'\n').nth(1) else {
        return false;
    };
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.len() < KEY_BYTES.div_ceil(3) * 4 || !line.len().is_multiple_of(4) {
        return false;
    }
    let mut prefix = [0; 6];
    if STANDARD.decode_slice(&line[..8], &mut prefix).is_err() || !header(&prefix) {
        return false;
    }
    let encoded = line
        .strip_suffix(b"==")
        .or_else(|| line.strip_suffix(b"="))
        .unwrap_or(line);
    encoded.len() * 3 / 4 >= KEY_BYTES
        && encoded[8..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
}

/// A content entry that cannot be safely loaded as data.
#[derive(Debug)]
pub enum ContentFileError {
    UnsupportedType(String),
    Executable(String),
    Read {
        name: String,
        source: std::io::Error,
    },
}

impl std::fmt::Display for ContentFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedType(name) => {
                write!(f, "{name}: unsupported file type in a content mod")
            }
            Self::Executable(name) => {
                write!(f, "{name}: unsupported executable file in a content mod")
            }
            Self::Read { name, source } => write!(f, "{name}: {source}"),
        }
    }
}

impl std::error::Error for ContentFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The shared content-file boundary. Refuse type/mode before opening a file,
/// then inspect its header even when its name has no executable extension.
pub(super) fn check_file<R: Read>(
    name: &str,
    mode: u32,
    open: impl FnOnce() -> std::io::Result<R>,
) -> Result<(), ContentFileError> {
    if !matches!(mode & 0o170000, 0 | 0o100000) {
        return Err(ContentFileError::UnsupportedType(name.into()));
    }
    let extension = name
        .rsplit(['/', '\\'])
        .next()
        .and_then(|n| n.rsplit_once('.'));
    let executable = extension.is_some_and(|(_, ext)| {
        [
            "exe", "dll", "so", "dylib", "class", "jar", "com", "bat", "cmd", "ps1", "wasm",
        ]
        .iter()
        .any(|kind| ext.eq_ignore_ascii_case(kind))
    });
    if executable || mode & 0o111 != 0 {
        return Err(ContentFileError::Executable(name.into()));
    }
    let error = |source| ContentFileError::Read {
        name: name.into(),
        source,
    };
    let mut file = open().map_err(error)?;
    let mut header = [0; 4];
    let mut read = 0;
    while read < header.len() {
        let size = file.read(&mut header[read..]).map_err(error)?;
        if size == 0 {
            break;
        }
        read += size;
    }
    let executable = matches!(
        &header,
        b"\x7fELF"
            | b"\0asm"
            | b"\xfe\xed\xfa\xce"
            | b"\xce\xfa\xed\xfe"
            | b"\xfe\xed\xfa\xcf"
            | b"\xcf\xfa\xed\xfe"
            | b"\xca\xfe\xba\xbe"
            | b"\xbe\xba\xfe\xca"
    ) || header.starts_with(b"MZ")
        || (header.starts_with(b"#!") && header[2] != b'[');
    if executable {
        return Err(ContentFileError::Executable(name.into()));
    }
    Ok(())
}

fn check_folder_file(root: &Path, name: &str) -> Result<(), String> {
    let path = root.join(name);
    let metadata = std::fs::symlink_metadata(&path).map_err(|e| format!("{name}: {e}"))?;
    let mode = if metadata.is_file() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            metadata.mode()
        }
        #[cfg(not(unix))]
        {
            0
        }
    } else {
        0o120000
    };
    check_file(name, mode, || std::fs::File::open(&path)).map_err(|e| e.to_string())
}

/// The `.smod` files and mod folders in `dir`, by name.
pub fn find(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "smod") || p.join(MANIFEST).is_file())
        .collect();
    found.sort();
    found
}

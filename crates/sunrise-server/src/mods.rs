//! Read-only checks for a managed server, before the engine or its mods are started.
use crate::engine;
use serde::Deserialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use sunrise_server_launch::{
    self as launch,
    config::{Config, ModId, Mods},
    mod_lock::{
        LOCK, LockedMod, MAX_LOCK, ModLock as Lock, Native, Reason, ReleaseTag, local_file,
        read_text,
    },
};

const MAX_IDENTITY: u64 = 16 << 10;

#[derive(Deserialize)]
struct Identity {
    file: String,
    sha256: String,
    identity: String,
}

/// The provenance and the library rows the engine loader reads from the same record.
#[derive(Deserialize)]
struct NativeRuntime {
    sunrise: String,
    libraries: Vec<RuntimeLibrary>,
}

#[derive(Deserialize)]
struct RuntimeLibrary {
    #[serde(rename = "mod")]
    id: String,
    #[serde(flatten)]
    stamp: Identity,
}

enum Repair<'a> {
    Mods,
    Mod(&'a ModId),
    Engine,
    Release(&'a ReleaseTag),
}

fn refuse_mod(row: &LockedMod, cause: &str) -> Refusal {
    Refusal::new(
        format!("{LOCK} says {} {}, but {cause}", row.id, row.version),
        Repair::Mod(&row.id),
    )
}

fn check_files(lock: &Lock, root: &Path, input: &Mods) -> Result<(), Refusal> {
    if lock.input != *input {
        return Err(
            "mods.lock was resolved from different [mods] settings than server.toml."
                .to_owned()
                .into(),
        );
    }
    for row in &lock.mods {
        let path = row.file.as_path();
        let file = local_file(root, path).map_err(|e| refuse_mod(row, &e))?;
        let hash = engine::sha256_file(&file)
            .map_err(|e| refuse_mod(row, &format!("{}: {e}", path.display())))?;
        if hash != row.sha256.as_str() {
            return Err(refuse_mod(
                row,
                &format!("{} has a different checksum.", path.display()),
            ));
        }
        if row.on && row.native.is_some() && !input.trust_native_code {
            return Err(refuse_mod(
                row,
                "native code is not trusted in server.toml.",
            ));
        }
    }
    Ok(())
}

fn check_native(lock: &Lock, root: &Path, program: &Path) -> Result<(), Refusal> {
    let build = lock.sunrise.as_str();
    let runtime = if let Some(row) = lock.mods.iter().find(|row| row.on && row.native.is_some()) {
        let error = |cause| refuse_mod(row, &format!("native-mods.json: {cause}"));
        let exe = fs::canonicalize(program).map_err(|e| error(e.to_string()))?;
        let record = exe.with_file_name("native-mods.json");
        let relative = record
            .strip_prefix(root)
            .map_err(|_| error("the native runtime is outside the server folder".into()))?;
        let path = local_file(root, relative).map_err(error)?;
        let text = read_text(&path, MAX_LOCK).map_err(error)?;
        let runtime: NativeRuntime =
            serde_json::from_str(&text).map_err(|e| error(e.to_string()))?;
        if runtime.sunrise != build {
            return Err(error(format!(
                "the runtime was built for {}, not {build}.",
                runtime.sunrise
            )));
        }
        Some((exe, runtime))
    } else {
        None
    };
    for row in &lock.mods {
        let Some(native) = &row.native else { continue };
        let path = row.file.as_path();
        let stamp = path.with_extension("identity.json");
        let error = |cause| refuse_mod(row, &format!("{}: {cause}", stamp.display()));
        let file = local_file(root, &stamp).map_err(error)?;
        let text = read_text(&file, MAX_IDENTITY).map_err(error)?;
        let identity: Identity = serde_json::from_str(&text).map_err(|e| error(e.to_string()))?;
        let hash_matches = match native {
            Native::Prebuilt(_) => identity.sha256 == row.sha256.as_str(),
            Native::Built(sdk) => {
                *sdk == lock.sunrise
                    && (identity.sha256.is_empty() || identity.sha256 == row.sha256.as_str())
            }
        };
        let mut parts = identity.identity.split_ascii_whitespace();
        let version = row.version.to_string();
        let identity_matches = parts
            .by_ref()
            .take(3)
            .eq([build, row.id.as_str(), &version])
            && parts
                .next()
                .is_some_and(|v| semver::Version::parse(v).is_ok())
            && parts.eq(["unwind"]);
        if Some(identity.file.as_ref()) != path.file_name() || !hash_matches || !identity_matches {
            return Err(error("does not match the library or engine build.".into()));
        }
        if let Some((exe, runtime)) = &runtime {
            let mut entries = runtime
                .libraries
                .iter()
                .filter(|lib| lib.id == row.id.as_str());
            let recorded = entries.next();
            let expected_place = exe.with_file_name(&identity.file);
            // The runtime reads the library beside the selected executable, not elsewhere in
            // the folder. Don't accept a lock that checks a different copy of the same file.
            let matches = recorded.is_some_and(|lib| {
                lib.stamp.file == identity.file
                    && lib.stamp.sha256 == row.sha256.as_str()
                    && lib.stamp.identity == identity.identity
                    && fs::canonicalize(root.join(path))
                        .is_ok_and(|actual| actual == expected_place)
            });
            if (row.on && (!matches || entries.next().is_some())) || (!row.on && recorded.is_some())
            {
                return Err(refuse_mod(
                    row,
                    "native-mods.json does not match the locked library or its on/off choice.",
                ));
            }
        }
    }
    Ok(())
}

fn write_list(lock: &Lock, mut out: impl Write, as_json: bool) -> std::io::Result<()> {
    if as_json {
        serde_json::to_writer(&mut out, lock)?;
        return writeln!(out);
    }
    if let Some(release) = &lock.release {
        writeln!(out, "Sunrise {release}")?;
    } else {
        writeln!(out, "Sunrise build {}", lock.sunrise)?;
    }
    for row in &lock.mods {
        let on = if row.on { "on" } else { "off" };
        let kind = match &row.native {
            Some(Native::Prebuilt(_)) => "native, prebuilt",
            Some(Native::Built(_)) => "native, built here",
            None => "content",
        };
        let why = match row.reason {
            Reason::Requested => "requested".into(),
            Reason::Pack => "from the pack".into(),
            Reason::Required => {
                let parents: Vec<_> = row.required_by.iter().map(|id| id.as_str()).collect();
                format!("needed by {}", parents.join(", "))
            }
        };
        writeln!(out, "  {} {}  {on}  {kind}  ({why})", row.id, row.version)?;
    }
    Ok(())
}

#[derive(Debug)]
pub struct Refusal(String);

impl Refusal {
    fn new(cause: String, repair: Repair<'_>) -> Self {
        let advice = match repair {
            Repair::Mods => "Put the mods back: sunrise-server mods update".to_owned(),
            Repair::Mod(id) => format!("Put it back: sunrise-server mods update {id}"),
            Repair::Engine => "Install a supported engine: sunrise-server update".to_owned(),
            Repair::Release(tag) => {
                format!("Bring them along: sunrise-server update --version {tag}")
            }
        };
        Self(format!("{cause}\n  {advice}"))
    }
}

impl From<String> for Refusal {
    fn from(cause: String) -> Self {
        Self::new(cause, Repair::Mods)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Returns the selected program when a lock is in use, so startup uses exactly what was checked.
pub fn check_start(
    dir: &Path,
    config: &Config,
    install: Option<&launch::Install>,
) -> Result<Option<PathBuf>, Refusal> {
    let Some(input) = &config.mods else {
        return Ok(None);
    };
    let root = fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let lock = Lock::read(&root)?;
    check_files(&lock, &root, input)?;
    sunrise_mod_install::selection::check_enabled(&root, &lock).map_err(Refusal)?;
    let program = launch::program_in(config, dir, install)
        .map_err(|cause| Refusal::new(cause, Repair::Engine))?;
    if let Some(expected) = &lock.release {
        let selected = install
            .filter(|install| {
                config.engine.path.is_empty()
                    && program.parent() == Some(install.programs().as_path())
            })
            .and_then(launch::Install::current)
            .and_then(|tag| ReleaseTag::try_from(tag).ok());
        match selected.as_ref() {
            Some(tag) if tag != expected => {
                return Err(Refusal::new(
                    format!(
                        "the mods were set up for Sunrise {expected}, but this server now runs {tag}."
                    ),
                    Repair::Release(tag),
                ));
            }
            None if !lock.mods.iter().any(|row| row.on && row.native.is_some()) => {
                eprintln!(
                    "sunrise-server: engine release unknown; cannot compare it with mods.lock ({expected})."
                );
            }
            _ => {}
        }
    }
    check_native(&lock, &root, &program)?;
    warn_takedowns(&root, &lock);
    Ok(Some(program))
}

mod manage;
pub use manage::{command, init};

pub fn read_lease(dir: &Path) -> Result<Option<fs::File>, Refusal> {
    sunrise_mod_install::transaction::read_lease(dir).map_err(|error| {
        Refusal(format!(
            "{error}\n  If interrupted: sunrise-server mods rollback"
        ))
    })
}

fn warn_takedowns(root: &Path, lock: &Lock) {
    let Ok(Some(index)) = sunrise_mod_install::registry::cached(root) else {
        return;
    };
    for row in &lock.mods {
        if let Some(revoked) = index.revoked(row.id.as_str(), row.sha256.as_str()) {
            eprintln!(
                "{} {} was taken down ({}). Remove it: sunrise-server mods remove {}",
                row.id, row.version, revoked.reason, row.id
            );
        }
    }
}

#[cfg(test)]
mod tests;

//! Cache acquisition and publication for the generated content root.
use super::*;

/// Takes the roots folder's lock, waiting (and saying so) while another process holds it. A
/// filesystem that can't lock goes on without one, as starts did before the lock.
fn lock_roots(roots: &Path) -> Result<Option<fs::File>> {
    fs::create_dir_all(roots)?;
    let path = roots.join(ROOTS_LOCK);
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let locked = match file.try_lock() {
        Ok(()) => Ok(()),
        Err(fs::TryLockError::WouldBlock) => {
            log::info!("Sunrise: waiting for another Sunrise process preparing mod content");
            file.lock()
        }
        Err(fs::TryLockError::Error(e)) => Err(e),
    };
    // Whatever a filesystem answers to a lock (none at all, or NFS without lockd), a start that
    // worked without the lock still starts.
    match locked {
        Ok(()) => Ok(Some(file)),
        Err(e) => {
            log::warn!(
                "Sunrise: {} can't be locked ({e}): preparing mod content without it",
                path.display()
            );
            Ok(None)
        }
    }
}

/// Builds a root using the owning load's condition registrations.
pub fn resource_root_with_conditions(
    base: &Path,
    jar_store: &Path,
    mods: &[ContentMod],
    cache: &Path,
    inventory: &sunrise_mod_package::enabled::EnabledMods,
    conditions: &sunrise_world::conditions::LoadConditions,
) -> Result<PathBuf> {
    let has_content = mods.iter().any(|m| {
        !m.jars.is_empty()
            || !m.content.is_empty()
            || !m.content.blocks.is_empty()
            || !m.content.items.is_empty()
            || !m.content.block_entity_types.is_empty()
    });
    if inventory.is_empty() && !has_content {
        return Ok(base.to_owned());
    }
    // Resolved once: the stamp and the build must name the same directory.
    let base = &real_base(base)?;
    let mut stamp = stamp(base, jar_store, mods, inventory)?;
    if conditions.has_registered() {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        stamp.hash(&mut hash);
        conditions.hash(&mut hash);
        // Handlers can read config that has no persistent fingerprint. Give each
        // build a fresh root, leaving every previously usable root untouched.
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
            .hash(&mut hash);
        stamp = format!("{:016x}{stamp}", hash.finish());
    }
    let roots = cache.join("mod-content");
    let out = roots.join(&stamp[..ROOT_NAME]);
    // Game folders that reach one stamp share its root: one process at a time checks, builds
    // or prunes in this cache, so none deletes or replaces a root another is reading.
    let _lock = lock_roots(&roots)?;
    if fs::read(out.join(STAMP)).is_ok_and(|s| s == stamp.as_bytes())
        && fs::read(out.join(sunrise_mod_package::enabled::MARKER)).is_ok_and(|bytes| {
            serde_json::from_slice::<sunrise_mod_package::enabled::EnabledMods>(&bytes)
                .is_ok_and(|loaded| &loaded == inventory)
        })
    {
        // A reused root warns as the build did.
        let warnings = fs::read_to_string(out.join(WARNINGS)).unwrap_or_default();
        warnings.lines().for_each(|w| log::warn!("Sunrise: {w}"));
        // Used again: kept another day. Marked beside the root, not in it: the pack overlay
        // laid over the root reads its files, and a rewritten one would rebuild it (#12403).
        let _ = fs::write(used_path(&out), b"");
        return Ok(out);
    }
    if has_content {
        for warning in build_with_conditions(base, jar_store, mods, &out, inventory, conditions)? {
            log::warn!("Sunrise: {warning}");
        }
    } else {
        // Code-only packages still own an inventory generation, but need no
        // registry-table rewrite or content scan. Existing tables stay linked.
        let overlay = Overlay::new(base, &out)?;
        overlay.write(
            sunrise_mod_package::enabled::MARKER,
            &serde_json::to_vec(inventory)?,
        )?;
        overlay.write(GENERATED_FILES, b"[]")?;
    }
    fs::write(out.join(STAMP), stamp)?;
    prune_roots(&roots, &out, KEEP_ROOTS);
    Ok(out)
}

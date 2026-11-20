//! Installed native code loading.
use mod_api::__private::game::Fields;
/// Load installed native code, register its dependency-ordered entries,
/// and start the engine. Keep the validated startup lease alive for the whole run.
pub fn main_with(
    mods: Vec<mod_api::ModEntry>,
    slots: &[crate::SlotListener],
    built: &'static [mod_api::__private::native::BuiltMod],
    startup: sunrise_client::entry::Startup,
) {
    sunrise_engine_slots::install_seal(mod_points::__private::seal)
        .expect("engine seal provider must be installed once before startup");
    // The title screen's Mods button: every game that loads mods has it, before the game seals
    // the title screen's buttons.
    if let Err(error) = sunrise_hooks::title::register(&crate::mods_button::MODS) {
        log::warn!("Sunrise: the title screen's Mods button did not register: {error}");
    }
    // Its label travels with it: the crate that supplies the row supplies its text.
    if let Err(error) = sunrise_hooks::lang::register(crate::mods_button::LANG) {
        log::warn!("Sunrise: the title screen's Mods label did not register: {error}");
    }
    // Platform instrumentation must precede registration and the first mod guard.
    if let Err(error) = mod_api::__private::initialize() {
        eprintln!("Sunrise: refusing startup because mod instrumentation is unavailable: {error}");
        std::process::exit(1);
    }
    if let Err(error) = sunrise_engine_slots::install_platform(sunrise_engine_slots::Platform {
        install_packages: mod_api::__private::install_packages,
        install_registration_data: mod_api::__private::install_registration_data,
        install_parts: mod_api::__private::install_parts,
        profiler_is_enabled: mod_api::__private::profiler::is_enabled,
        profiler_start: mod_api::__private::profiler::start,
        profiler_stop: mod_api::__private::profiler::stop,
        profiler_summary: mod_api::__private::profiler::summary,
        profiler_firefox_profile: mod_api::__private::profiler::firefox_profile,
    }) {
        eprintln!("Sunrise: {error}");
        std::process::exit(1);
    }
    let exe = std::env::current_exe();
    let native: Result<_, Box<dyn std::error::Error + Send + Sync>> = exe
        .as_ref()
        .map_err(|e| e.to_string().into())
        .and_then(|exe| {
            sunrise_mod_package::runtime::Runtime::load(exe, built).map_err(Into::into)
        });
    let native = match native {
        Ok(native) => native,
        Err(error) => {
            eprintln!("Sunrise: {error}");
            std::process::exit(1);
        }
    };
    mod_render::render::install();
    sunrise_client::entry::start(
        native,
        |native, launch| {
            // Installed code dispatch follows the resolved package order.
            let code_manifests: Vec<_> = native
                .packages()
                .iter()
                .map(|p| &p.manifest)
                .filter(|m| mods.iter().any(|entry| entry.fields().id == m.id))
                .collect();
            assert_eq!(
                code_manifests.len(),
                mods.len(),
                "loaded code has checked manifests"
            );
            let side = launch.installation_side();
            let declared: Vec<_> = code_manifests
                .iter()
                .map(|m| sunrise_mod_package::dependencies::from_manifest(&m.dependencies, side))
                .collect();
            let inputs: Vec<_> = code_manifests
                .iter()
                .zip(&declared)
                .map(|(m, dependencies)| sunrise_mod_package::dependencies::Mod {
                    id: &m.id,
                    version: &m.version,
                    has_api: true,
                    code: true,
                    dependencies,
                })
                .collect();
            let code_ids: std::collections::HashSet<_> =
                mods.iter().map(|entry| entry.fields().id).collect();
            let settings = sunrise_mod_package::mod_list::ModList::load(&launch.context.game_dir);
            let content: Vec<_> = sunrise_mod_package::selection::content_packages(
                &launch.context.game_dir,
                &settings.enabled,
                &launch.context.dev_mods,
            )
            .collect();
            let manifests: Vec<_> = code_manifests
                .iter()
                .copied()
                .chain(content.iter().map(|source| &source.package.manifest))
                .collect();
            let order = sunrise_mod_package::dependencies::Overrides::read(
                &launch
                    .context
                    .game_dir
                    .join(sunrise_mod_package::mod_list::DIR),
            )
            .and_then(|overrides| {
                let resolution = sunrise_mod_package::dependencies::resolve_manifests(
                    &manifests, side, &overrides,
                )
                .map_err(|error| error.to_string())?;
                sunrise_mod_package::dependencies::resolve_links(&inputs)
                    .map_err(|error| format!("code link requirements refused: {error}"))?;
                Ok(resolution)
            })
            .unwrap_or_else(|error| {
                eprintln!("Sunrise: mod load refused: {error}");
                sunrise_client::logging::flush();
                std::process::exit(1);
            });
            for warning in &order.warnings {
                log::warn!("Sunrise: {warning}");
            }
            // What every level.dat this game saves records (`fml.LoadingModList`).
            sunrise_client::save::level_mods::set_loaded(Some(
                order
                    .order
                    .iter()
                    .filter(|&&i| code_ids.contains(manifests[i].id.as_str()))
                    .map(|&i| sunrise_client::save::level_mods::ModRecord {
                        id: manifests[i].id.clone(),
                        version: manifests[i].version.to_string(),
                    })
                    .collect(),
            ));
            let mut entries: std::collections::BTreeMap<_, _> = mods
                .into_iter()
                .map(|entry| (entry.fields().id, entry))
                .collect();
            let mods: Vec<_> = order
                .order
                .into_iter()
                .filter(|&i| code_ids.contains(manifests[i].id.as_str()))
                .map(|i| entries.remove(manifests[i].id.as_str()).unwrap())
                .collect();
            let ids: Vec<_> = mods.iter().map(|entry| entry.fields().id).collect();
            mod_api::__private::install(mods.into());
            for &id in &ids {
                for slot in slots.iter().filter(|slot| slot.id() == id) {
                    if let Err(error) = mod_points::register(slot) {
                        log::error!("Sunrise: {id}: {error}");
                        sunrise_client::logging::flush();
                        std::process::exit(1);
                    }
                }
            }
        },
        startup,
    );
}

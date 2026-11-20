//! Cargo-facing generation: the mod API's and the game's generated files.
use super::{Catalog, Target, free_function_source, host, sources};
use quote::ToTokens;
use std::{fs, path::Path};
mod vocab_boundary;

fn write_vocab_output(
    out_dir: &Path,
    name: &str,
    code: &str,
    inventory: &vocab_boundary::Inventory,
    sources: &std::collections::BTreeSet<std::path::PathBuf>,
) -> Result<(), String> {
    vocab_boundary::validate_with_modules(name, code, inventory, sources)
        .map_err(|e| format!("{name}: {e}"))?;
    write_if_changed(out_dir.join(name), code).map_err(|e| e.to_string())
}

/// Preserve output timestamps when generation produces identical bytes.
pub fn write_if_changed(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    let path = path.as_ref();
    let bytes = bytes.as_ref();
    match fs::read(path) {
        Ok(previous) if previous == bytes => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::write(path, bytes)
}

/// Generate only this crate's files, tracking every source read by the generator.
/// `root` is supplied at run time by the consuming crate's build script.
pub fn generate(root: &Path, out_dir: &Path, target: Target) -> Result<(), String> {
    let game = matches!(&target, Target::Client);
    track(root, game)?;
    let inventory = if matches!(target, Target::Vocab) {
        let path = root.join("tools/layering/mod_vocab_state.json");
        crate::rerun_if_changed(&path.display());
        vocab_boundary::inventory(root)?
    } else {
        vocab_boundary::Inventory::new()
    };
    let vocab_sources = if matches!(target, Target::Vocab) {
        vocab_boundary::modules::sources(root)?
    } else {
        std::collections::BTreeSet::new()
    };
    if matches!(&target, Target::Client) {
        crate::rerun_if_staged(root);
    }
    let generated = sources(root, game.then_some(super::FillMode::Require))?;
    let outputs = match target {
        Target::Client => vec![
            (
                "native_hook_modules.rs",
                sunrise_game_discovery::native_hook_modules(root)?,
            ),
            ("lan_action_modules.rs", {
                let directory = root.join("crates/sunrise-client/src/game/network/lan_actions");
                sunrise_game_discovery::lan_action_modules(
                    &directory,
                    Catalog::modules(&directory).map_err(|e| e.to_string())?,
                )?
            }),
            ("game_host_modules.rs", game_host_modules(root)?),
            ("mod_api_fire_generated.rs", generated.fire),
            // The mob tick's goal read view (#1077), included beside the
            // engine's `GoalCx` in `mobs/part_goals.rs`.
            ("mod_api_goal_view_generated.rs", generated.goal_view),
        ],
        Target::Api | Target::Vocab | Target::Marks => {
            if matches!(target, Target::Api) {
                generate_api_parts(root, out_dir)?;
            }
            if matches!(target, Target::Vocab) {
                write_vocab_output(
                    out_dir,
                    super::parts::OUTPUT,
                    &super::parts::source(root)?,
                    &inventory,
                    &vocab_sources,
                )?;
                for (name, code) in super::exports::sources(root)? {
                    write_vocab_output(out_dir, &name, &code, &inventory, &vocab_sources)?;
                }
            }
            let hooks = generated.hooks;
            let included = [
                (host::AREAS_OUTPUT, generated.areas.as_str()),
                ("mod_api_generated.rs", hooks.as_str()),
                ("mod_api_generated_host.rs", generated.host.as_str()),
            ];
            let (providers, sources) = super::capability_providers::generate(root, &included)?;
            // The resolver recurses through the API's types: give it a deep stack.
            let generated_wire = std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .stack_size(64 << 20)
                    .spawn_scoped(scope, || {
                        super::wire::generate_with_providers(root, &included, &providers)
                    })
                    .map_err(|e| e.to_string())?
                    .join()
                    .map_err(|_| "generating the wire impls panicked".to_string())?
            })?;
            for path in sources.iter().chain(&generated_wire.sources) {
                crate::rerun_if_changed(&path.display());
            }
            let outputs = vec![
                (MARKS_OUTPUT, generated_wire.marks),
                (host::AREAS_OUTPUT, generated.areas),
                ("mod_api_generated.rs", hooks),
                ("mod_api_generated_host.rs", generated.host),
                ("capability_providers_generated.rs", providers),
                ("wire_generated.rs", generated_wire.wire),
                (GUEST_OUTPUT, generated_wire.guest),
            ];
            outputs
                .into_iter()
                // Vocab keeps the mod side's guest transport as well, for `sunrise-mod-api`'s build to
                // take (`generate_api_parts`): it comes from the sources Vocab has just generated.
                .filter(|(name, _)| match target {
                    Target::Api => *name == GUEST_OUTPUT,
                    Target::Vocab => *name != MARKS_OUTPUT,
                    Target::Marks => *name == MARKS_OUTPUT,
                    Target::Client => unreachable!(),
                })
                .collect()
        }
    };
    for (name, code) in outputs {
        let code = generated_source(root, &target, name, code)?;
        if matches!(target, Target::Vocab) && name != GUEST_OUTPUT {
            write_vocab_output(out_dir, name, &code, &inventory, &vocab_sources)?;
        } else {
            write_if_changed(out_dir.join(name), &code).map_err(|e| e.to_string())?;
        }
        syn::parse_file(&code).map_err(|e| format!("generated Rust: {name}: {e}"))?;
    }
    Ok(())
}

/// The handler traits' import functions, which `sunrise-mod-marks` includes: the one place the
/// imports' unsafe code lives, so the vocabulary holds none.
pub const MARKS_OUTPUT: &str = "handler_marks_generated.rs";

/// The mod side's WebAssembly guest transport, which `sunrise-mod-api` includes.
pub const GUEST_OUTPUT: &str = "wire_guest_generated.rs";

/// `sunrise-mod-api`'s own generated file: the modules and re-exports of its Part kinds. Its guest
/// transport ([`GUEST_OUTPUT`]) comes from `sunrise-mod-vocab`'s build, which generates the same sources:
/// the API's build script copies it instead of parsing the catalog, the vocabulary and the game again.
pub fn generate_api_parts(root: &Path, out_dir: &Path) -> Result<(), String> {
    crate::rerun_if_changed(&root.join("crates/sunrise-mod-api/src/parts").display());
    write_if_changed(
        out_dir.join("parts_registration_modules.rs"),
        super::parts::registration_source(root)?,
    )
    .map_err(|e| e.to_string())
}

fn generated_source(
    root: &Path,
    target: &Target,
    name: &str,
    code: String,
) -> Result<String, String> {
    if matches!(target, Target::Client)
        && matches!(
            name,
            "mod_api_fire_generated.rs" | "mod_api_goal_view_generated.rs"
        )
        && root.join(".sunrise-source-staged").is_file()
    {
        super::internals::widen_generated(Path::new(name), code)
    } else {
        Ok(code)
    }
}

#[cfg(test)]
mod generated_visibility_tests {
    use super::*;

    #[test]
    fn staged_game_helpers_are_widened_but_source_helpers_are_unchanged() {
        let root =
            std::env::temp_dir().join(format!("fnr-generated-visibility-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let input = "impl Game<'_> { pub(super) fn fire(&mut self) {} }";
        for name in [
            "mod_api_fire_generated.rs",
            "mod_api_goal_view_generated.rs",
        ] {
            assert_eq!(
                generated_source(&root, &Target::Client, name, input.into()).unwrap(),
                input
            );
        }
        fs::write(root.join(".sunrise-source-staged"), "").unwrap();
        for name in [
            "mod_api_fire_generated.rs",
            "mod_api_goal_view_generated.rs",
        ] {
            assert_eq!(
                generated_source(&root, &Target::Client, name, input.into()).unwrap(),
                "impl Game<'_> { pub fn fire(&mut self) {} }"
            );
            assert_eq!(
                generated_source(&root, &Target::Vocab, name, input.into()).unwrap(),
                input
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

/// The game's side of the host traits: one module for each area file of `game/host/`.
pub(crate) fn game_host_modules(root: &Path) -> Result<String, String> {
    let mut modules = sunrise_game_discovery::host_modules(root)?;
    modules.push_str(&format!(
        "mod generated {{\n{}\n}}\n",
        host::game_forwarders(root)?
    ));
    Ok(modules)
}

/// Generate the native-only raw render records, without trait dispatch or subscriptions.
pub fn generate_render(root: &Path, out_dir: &Path) -> Result<(), String> {
    let declarations = root.join(super::catalog::DECLARATIONS);
    crate::rerun_if_changed(&declarations.display());
    for module in Catalog::modules(&declarations)? {
        crate::rerun_if_changed(&declarations.join(format!("{module}.rs")).display());
    }
    crate::rerun_if_changed(
        &root
            .join("crates/sunrise-mod-render/src/render.rs")
            .display(),
    );
    // The render hooks' records, each as the dispatch emits it.
    let records = format!(
        "use super::*;\n{}",
        super::dispatch::render(root, &Catalog::load(root)?, true)?.render_dispatch
    );
    syn::parse_file(&records).map_err(|error| error.to_string())?;
    write_if_changed(out_dir.join("native_render_generated.rs"), records)
        .map_err(|error| error.to_string())
}

#[test]
fn render_records_need_no_game_sources() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = workspace.join(format!("target/render-records-only-{}", std::process::id()));
    let declarations = root.join(super::catalog::DECLARATIONS);
    fs::create_dir_all(&declarations).unwrap();
    for entry in fs::read_dir(workspace.join(super::catalog::DECLARATIONS)).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            fs::copy(&path, declarations.join(path.file_name().unwrap())).unwrap();
        }
    }
    let raw = "crates/sunrise-mod-render/src/render.rs";
    fs::create_dir_all(root.join(raw).parent().unwrap()).unwrap();
    fs::copy(workspace.join(raw), root.join(raw)).unwrap();
    assert!(!root.join("crates/sunrise-client").exists());
    generate_render(&root, &root).unwrap();
    let generated = fs::read_to_string(root.join("native_render_generated.rs")).unwrap();
    let file = syn::parse_file(&generated).unwrap();
    assert_eq!(
        file.items
            .iter()
            .filter(|item| matches!(item, syn::Item::Struct(_)))
            .count(),
        5
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "daily"]
fn listed_render_slots_have_eligible_points() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    super::points::game_points(&root).unwrap();
}

/// The `mod_api` types with a `Wire` layout ([`super::wire`]), for point records.
/// A source tree without the mod API (a generator fixture's) has none.
pub(crate) fn wire_types(root: &Path) -> Result<std::collections::BTreeSet<String>, String> {
    if !root.join("crates/sunrise-mod-vocab/src/hooks").is_dir() {
        return Ok(Default::default());
    }
    let generated = sources(root, None)?;
    wire_types_from_sources(root, &generated)
}

/// Resolve Wire layouts from already validated, immutable API sources.
/// The caller has established that the mod API directory exists.
pub(super) fn wire_types_from_sources(
    root: &Path,
    generated: &super::Sources,
) -> Result<std::collections::BTreeSet<String>, String> {
    let included = [
        (host::AREAS_OUTPUT, generated.areas.as_str()),
        ("mod_api_generated.rs", generated.hooks.as_str()),
        ("mod_api_generated_host.rs", generated.host.as_str()),
    ];
    // The resolver recurses through the API's types: give it a deep stack.
    let types = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn_scoped(scope, || super::wire::owned_types(root, &included))
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "reading the wire types panicked".to_string())?
    })?;
    Ok(types)
}

/// `sunrise-mod-points`' generated file: each point's record, `PointId` and typed function
/// constructors, from the point catalog ([`super::points::catalog`]).
pub fn generate_points(root: &Path, out_dir: &Path) -> Result<(), String> {
    // Phase durations for CI's build report (points-timing.txt beside the output); never read back.
    let started = std::time::Instant::now();
    let mut timing = String::new();
    let mut phase = |name: &str| {
        timing.push_str(&format!("{name} {:.2}s\n", started.elapsed().as_secs_f64()));
    };
    let catalog = super::points::catalog(root, listener_fixture())?;
    let selected = selected_points(&catalog.points)?;
    phase("catalog");
    let code = super::points::records_selected(&catalog.points, selected.as_ref())
        + &format!(
            "/// Whether this build compiled the points' sites into the game: only the prebuilt game, built from a staged copy of the source, has them. A build without them refuses every listener at load.\npub const STAGED: bool = {};\n",
            catalog.sited
        );
    phase("records");
    let generated = syn::parse_file(&code).map_err(|e| format!("generated Rust: points: {e}"))?;
    let code = short_lines(generated);
    phase("format");
    write_if_changed(out_dir.join("points_generated.rs"), code).map_err(|e| e.to_string())?;
    phase("written");
    fs::write(out_dir.join("points-timing.txt"), timing).map_err(|e| e.to_string())
}

/// Generate the engine-owned point vocabulary and typed dispatch, from the point catalog.
pub fn generate_engine_points(root: &Path, out_dir: &Path) -> Result<(), String> {
    let catalog = super::points::catalog(root, listener_fixture())?;
    let selected = selected_points(&catalog.points)?;
    let code = super::points::engine_records(&catalog.points, selected.as_ref());
    let generated = syn::parse_file(&code).map_err(|e| format!("engine points: {e}"))?;
    let code = short_lines(generated);
    write_if_changed(out_dir.join("engine_points.rs"), code).map_err(|e| e.to_string())
}

/// Whether the build script's package builds with its `listener-fixture` feature.
fn listener_fixture() -> bool {
    std::env::var_os("CARGO_FEATURE_LISTENER_FIXTURE").is_some()
}

/// The places `SUNRISE_POINT_NAMES` selects (a wasm mod's build lists the ones it listens at),
/// each of them a place of `points`; none when it is unset.
fn selected_points(
    points: &[super::points::GamePoint],
) -> Result<Option<std::collections::BTreeSet<String>>, String> {
    println!("cargo:rerun-if-env-changed=SUNRISE_POINT_NAMES");
    let Ok(json) = std::env::var("SUNRISE_POINT_NAMES") else {
        return Ok(None);
    };
    let selected: std::collections::BTreeSet<String> =
        serde_json::from_str(&json).map_err(|error| format!("SUNRISE_POINT_NAMES: {error}"))?;
    let places: std::collections::BTreeSet<String> = points
        .iter()
        .flat_map(|point| [point.name.clone(), format!("{}_return", point.name)])
        .collect();
    if let Some(unknown) = selected.iter().find(|name| !places.contains(*name)) {
        return Err(format!(
            "SUNRISE_POINT_NAMES: {unknown} is no place of this build's {} points: the catalog the mod was checked against is another build's",
            points.len()
        ));
    }
    Ok(Some(selected))
}

/// `file`'s tokens as text with a line break after every item and after every `,` or `;` inside
/// braces and brackets (a `match`, a list, a block), and no other change: a single multi-megabyte
/// line makes Clippy's source lookups, `clippy::four_forward_slashes` above all, prohibitively slow.
fn short_lines(file: syn::File) -> String {
    use proc_macro2::{Delimiter, Spacing, TokenTree};
    fn push(code: &mut String, stream: proc_macro2::TokenStream, breaks: bool) {
        for tree in stream {
            match tree {
                TokenTree::Group(group) => {
                    let (open, close, inner) = match group.delimiter() {
                        Delimiter::Parenthesis => ("( ", ") ", false),
                        Delimiter::Brace => ("{ ", "} ", true),
                        Delimiter::Bracket => ("[ ", "] ", true),
                        Delimiter::None => ("", "", breaks),
                    };
                    code.push_str(open);
                    push(code, group.stream(), inner);
                    code.push_str(close);
                }
                TokenTree::Punct(punct) => {
                    code.push(punct.as_char());
                    match punct.spacing() {
                        Spacing::Joint => {}
                        Spacing::Alone if breaks && matches!(punct.as_char(), ',' | ';') => {
                            code.push('\n')
                        }
                        Spacing::Alone => code.push(' '),
                    }
                }
                tree => {
                    code.push_str(&tree.to_string());
                    code.push(' ');
                }
            }
        }
    }
    let mut code = String::new();
    for item in file.items {
        push(&mut code, item.to_token_stream(), true);
        code.push('\n');
    }
    code
}

/// Tracks what the hooks' sources are generated from: the vocabulary's declarations, exports
/// and parts, and with `game` (the game's own build, which checks the game against them) every
/// game file and each target of another crate.
pub(super) fn track(root: &Path, game: bool) -> Result<(), String> {
    fn directory(path: &Path, extension: &str, recursive: bool) -> Result<(), String> {
        // Track the directory as well: adding a helper source or fragment changes the inputs.
        crate::rerun_if_changed(&path.display());
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() && recursive {
                directory(&path, extension, recursive)?;
            } else if path.extension().is_some_and(|ext| ext == extension) {
                crate::rerun_if_changed(&path.display());
            }
        }
        Ok(())
    }
    crate::rerun_if_changed(&root.join("crates/sunrise-mod-vocab/src").display());
    for retired in super::catalog::RETIRED {
        // Only while present: Cargo reruns a build script on every build for a path that doesn't
        // exist. One that appears fails the next generation, and every fresh build, such as CI's.
        let path = root.join(retired);
        if path.exists() {
            crate::rerun_if_changed(&path.display());
        }
    }
    directory(&root.join(super::catalog::DECLARATIONS), "rs", false)?;
    directory(&root.join(super::exports::DIRECTORY), "rs", false)?;
    directory(
        &root.join("crates/sunrise-mod-vocab/src/parts"),
        "rs",
        false,
    )?;
    if !game {
        return Ok(());
    }
    directory(&root.join("crates/sunrise-client/src/game"), "rs", true)?;
    crate::rerun_if_changed(&root.join("crates/sunrise-client/src/game.rs").display());
    // Render hooks' functions are checked by `sunrise-mod-render`'s own build.
    for hook in Catalog::load(root)?.hooks {
        if super::dispatch::is_render(&hook) {
            continue;
        }
        if let Some((krate, path)) = free_function_source(&hook.target) {
            let (module, _) = path.rsplit_once("::").ok_or("target missing module")?;
            let path = root
                .join("crates")
                .join(krate)
                .join("src")
                .join(format!("{}.rs", module.replace("::", "/")));
            crate::rerun_if_changed(&path.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The output differs from the token stream's own text only in whitespace, and no line holds
    /// more than a list's element.
    #[test]
    fn short_lines_keep_the_tokens_and_break_lists() {
        let source = "pub enum Id { A, B, C } impl Id { pub fn n(self) -> u8 { let t = [1, 2, 3]; match self { Id::A => { f(1, 2); 1 } _ => 2 } } } const S: &str = \"a, b; c\"; fn g<'a>(x: &'a u8) -> Vec<::std::string::String> { vec![] }";
        let file = syn::parse_file(source).unwrap();
        let text = short_lines(file.clone());
        let original = file.to_token_stream().to_string();
        let reparsed = syn::parse_file(&text)
            .unwrap()
            .to_token_stream()
            .to_string();
        assert_eq!(reparsed, original);
        assert!(text.lines().count() > 10);
        assert!(text.contains("\"a, b; c\""));
    }
}

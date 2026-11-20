//! Native hook targets come from definitions, not declaration file names.
use crate::catalog::{Catalog, PartKind};
use quote::ToTokens;
use std::{collections::BTreeMap, fs, path::Path};

fn definitions(root: &Path) -> Result<BTreeMap<String, (String, syn::Signature)>, String> {
    let mut targets = BTreeMap::new();
    for path in sunrise_game_discovery::native_hook_files(root)? {
        let area = path.file_stem().unwrap().to_str().unwrap();
        let file = syn::parse_file(&fs::read_to_string(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        for item in file.items {
            let syn::Item::Impl(item) = item else {
                continue;
            };
            if item.trait_.is_some() {
                continue;
            }
            let ty = item.self_ty.to_token_stream().to_string().replace(' ', "");

            for member in item.items {
                let syn::ImplItem::Fn(method) = member else {
                    continue;
                };
                let name = method.sig.ident.to_string();
                if !name.starts_with("native_") {
                    continue;
                }
                let target = format!("sunrise_client::game::native_hooks::{area}::{ty}::{name}");
                if targets
                    .insert(format!("{ty}::{name}"), (target, method.sig))
                    .is_some()
                {
                    return Err(format!("duplicate native hook definition: {name}"));
                }
            }
        }
    }
    Ok(targets)
}

pub(crate) const EFFECT_TICK_TARGET: &str = "mod_api::parts::effects::dispatch_mob_effect_tick";

/// The game's own build: every `native_hooks` target is defined where its declaration says, and
/// each effect tick hook is declared against the vocabulary's paired dispatcher.
pub(crate) fn check(root: &Path, catalog: &Catalog) -> Result<(), String> {
    let targets = definitions(root)?;
    let mut checked_effect_tick_target = false;
    for hook in &catalog.hooks {
        if hook
            .scope
            .as_ref()
            .is_some_and(|scope| scope.kind == PartKind::Effect)
        {
            if !matches!(
                hook.name.as_str(),
                "should_apply_effect_tick_this_tick" | "apply_effect_tick"
            ) || hook.target != EFFECT_TICK_TARGET
                || hook.call.is_empty()
            {
                return Err(format!(
                    "{}: an effect tick hook must declare its call to {EFFECT_TICK_TARGET}",
                    hook.name
                ));
            }
            if !checked_effect_tick_target {
                let path = root.join("crates/sunrise-mod-vocab/src/parts/effects.rs");
                let source =
                    fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let parsed = syn::parse_file(&source).map_err(|e| e.to_string())?;
                if !parsed.items.iter().any(|item| matches!(item, syn::Item::Fn(f) if f.sig.ident == "dispatch_mob_effect_tick")) {
                    return Err(format!("{}: no definition for {EFFECT_TICK_TARGET}", hook.name));
                }
                checked_effect_tick_target = true;
            }
            continue;
        }
        let Some(tail) = hook
            .target
            .strip_prefix("sunrise_client::game::native_hooks::")
        else {
            continue;
        };
        let name = tail.split("::").skip(1).collect::<Vec<_>>().join("::");
        let Some((target, _)) = targets.get(&name) else {
            return Err(format!("{}: no native definition", hook.name));
        };
        if *target != hook.target {
            return Err(format!(
                "{}: declared at {}, defined at {target}: give its declaration `target = \"{target}\"`",
                hook.name, hook.target
            ));
        }
    }
    Ok(())
}

/// Writes into each hook declaration what the hooks' sources are generated from where the game
/// is not read: its target function's signature (`native`), its `target` when the native
/// function lives in another `native_hooks` module than its declaration's, and `direct` for a
/// target outside the game that calls its own dispatch. Returns the files it changed.
pub(crate) fn declare(root: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let definitions = definitions(root)?;
    let mut methods = Vec::new();
    for path in sunrise_game_discovery::native_hook_files(root)? {
        let file = syn::parse_file(&fs::read_to_string(&path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        for item in file.items {
            if let syn::Item::Impl(item) = item {
                methods.extend(crate::impl_functions(item));
            }
        }
    }
    let catalog = Catalog::load(root).map_err(|e| e.to_string())?;
    let mut wanted: BTreeMap<String, Vec<(&str, String)>> = BTreeMap::new();
    for hook in &catalog.hooks {
        let name = hook.name.as_str();
        if hook.target == EFFECT_TICK_TARGET {
            continue;
        }
        // A render hook's records are generated by `sunrise-mod-render`'s build from its own
        // functions: it needs no `native`, only `direct` where its function dispatches.
        let render = crate::dispatch::is_render(hook);
        let mut target = hook.target.clone();
        if let Some(tail) = target.strip_prefix("sunrise_client::game::native_hooks::") {
            let key = tail.split("::").skip(1).collect::<Vec<_>>().join("::");
            let (defined, _) = definitions
                .get(&key)
                .ok_or_else(|| format!("{name}: no native definition"))?;
            if *defined != target {
                target = defined.clone();
                wanted
                    .entry(name.into())
                    .or_default()
                    .push(("target", target.clone()));
            }
        }
        let resolved = crate::catalog::Hook {
            target: target.clone(),
            ..hook.clone()
        };
        let external = crate::free_function_source(&target);
        let function = if render {
            name.to_owned()
        } else {
            format!("native_{name}")
        };
        let defined = crate::dispatch::defined_function(
            root,
            &resolved,
            &function,
            external.as_ref(),
            &methods,
        )?;
        let native = signature_text(&defined.sig);
        if !render
            && hook.native.as_deref().map(|text| {
                syn::parse_str::<syn::Signature>(text)
                    .map(|sig| crate::dispatch::signature_tokens(&sig))
                    .ok()
            }) != Some(Some(crate::dispatch::signature_tokens(&defined.sig)))
        {
            wanted
                .entry(name.into())
                .or_default()
                .push(("native", native));
        }
        if let Some((krate, _)) = &external
            && *krate != "sunrise-client-render"
            && !hook.direct
            && crate::has_call(&defined.block, &format!("dispatch_{name}"))
        {
            wanted
                .entry(name.into())
                .or_default()
                .push(("direct", String::new()));
        }
    }
    let mut changed = Vec::new();
    let directory = root.join(crate::catalog::DECLARATIONS);
    for module in Catalog::modules(&directory).map_err(|e| e.to_string())? {
        let path = directory.join(format!("{module}.rs"));
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let rewritten =
            declare_in(&text, &wanted).map_err(|e| format!("{}: {e}", path.display()))?;
        if rewritten != text {
            fs::write(&path, rewritten).map_err(|e| format!("{}: {e}", path.display()))?;
            changed.push(path);
        }
    }
    Ok(changed)
}

/// `text` with the arguments `wanted` names added to (or, for `native` and `target`, replaced
/// in) each listed hook's `#[hook(...)]`, one argument a line.
fn declare_in(
    text: &str,
    wanted: &BTreeMap<String, Vec<(&str, String)>>,
) -> Result<String, String> {
    use syn::spanned::Spanned;
    let file = syn::parse_file(text).map_err(|e| e.to_string())?;
    let lines: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let offset = |at: proc_macro2::LineColumn| {
        lines[at.line - 1]
            + text[lines[at.line - 1]..]
                .chars()
                .take(at.column)
                .map(char::len_utf8)
                .sum::<usize>()
    };
    let mut edits = Vec::new();
    for item in &file.items {
        let syn::Item::Trait(declarations) = item else {
            continue;
        };
        if declarations.ident != "Hooks" {
            continue;
        }
        for member in &declarations.items {
            let syn::TraitItem::Fn(function) = member else {
                continue;
            };
            let Some(added) = wanted.get(&function.sig.ident.to_string()) else {
                continue;
            };
            let attr = function
                .attrs
                .iter()
                .find(|attr| attr.path().is_ident("hook"))
                .ok_or_else(|| format!("{}: no #[hook(...)]", function.sig.ident))?;
            let arguments = attr
                .parse_args_with(
                    syn::punctuated::Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated,
                )
                .map_err(|e| format!("{}: {e}", function.sig.ident))?;
            let mut rendered: Vec<String> = arguments
                .iter()
                .filter(|argument| {
                    !added
                        .iter()
                        .any(|(key, _)| *key != "direct" && argument.path.is_ident(key))
                })
                .map(|argument| {
                    text[offset(argument.span().start())..offset(argument.span().end())].to_owned()
                })
                .collect();
            for (key, value) in added {
                rendered.push(match *key {
                    "direct" => "direct = true".to_owned(),
                    key => format!("{key} = {value:?}"),
                });
            }
            let start = offset(attr.span().start());
            let indent = &text[lines[attr.span().start().line - 1]..start];
            let body = rendered
                .iter()
                .map(|argument| format!("{indent}    {argument}"))
                .collect::<Vec<_>>()
                .join(",\n");
            edits.push((
                start,
                offset(attr.span().end()),
                format!("#[hook(\n{body}\n{indent})]"),
            ));
        }
    }
    let mut out = text.to_owned();
    for (start, end, replacement) in edits.into_iter().rev() {
        out.replace_range(start..end, &replacement);
    }
    Ok(out)
}

/// A signature as a declaration's `native` writes it: on one line, as rustfmt would print it.
fn signature_text(sig: &syn::Signature) -> String {
    let declaration: syn::ItemTrait = syn::parse_quote!(trait T { #sig; });
    let text = prettyplease::unparse(&syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: vec![syn::Item::Trait(declaration)],
    });
    let line = text
        .lines()
        .skip(1)
        .take_while(|line| *line != "}")
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    line.trim_end_matches(';')
        .replace("( ", "(")
        .replace(", )", ")")
        .replace(",)", ")")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_signature_still_requires_its_native_definition() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let declared = Catalog::load(&root).unwrap();
        let mut hook = declared
            .hooks
            .into_iter()
            .find(|hook| !hook.call.is_empty())
            .unwrap();
        hook.target =
            "sunrise_client::game::native_hooks::base::Game::native_missing_fixture".into();
        hook.scope = None;
        let catalog = Catalog {
            hooks: vec![hook],
            draw_contexts: Vec::new(),
            general_notes: None,
        };
        assert!(
            check(&root, &catalog)
                .unwrap_err()
                .contains("no native definition")
        );
    }

    #[test]
    #[ignore = "daily"]
    fn every_catalog_target_is_defined_where_declared_and_a_moved_one_is_rejected() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut catalog = Catalog::load(&root).unwrap();
        // Dispatch reads named game methods and free render functions through the same source
        // parsers the game's build checks them with (including crate aliases).
        crate::sources(&root, Some(crate::FillMode::Require)).unwrap();
        crate::dispatch::render(&root, &catalog, true).unwrap();
        let hook = catalog
            .hooks
            .iter_mut()
            .find(|h| h.name == "anvil_craft_post")
            .unwrap();
        assert!(hook.target.contains("::base::"));
        hook.target = hook.target.replace("::base::", "::elsewhere::");
        assert!(
            check(&root, &catalog)
                .unwrap_err()
                .contains("defined at sunrise_client::game::native_hooks::base::")
        );
    }
}

#[cfg(test)]
#[test]
fn stack_count_rejects_non_stack_outputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut catalog = Catalog::load(&root).unwrap();
    let hook = catalog
        .hooks
        .iter_mut()
        .find(|h| h.name == "can_equip")
        .unwrap();
    hook.stack_count = Some(crate::catalog::StackCount::Trim);
    let Err(error) = crate::dispatch::emit(
        &root,
        catalog.hooks.iter().filter(|h| h.name == "can_equip"),
        None,
        &catalog.draw_contexts,
    ) else {
        panic!("a stack count on a non-stack output is accepted");
    };
    assert!(error.contains("mutable Vec<ItemStack>"), "{error}");
}

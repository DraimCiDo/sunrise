//! Operator commands for the shared server mod installer. Stdout is the result (or JSON);
//! confirmations, warnings and download progress go to stderr.
use super::{Lock, check_files, warn_takedowns, write_list};
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::Path,
};
use sunrise_mod_install::{
    Side,
    index::{Index, Kind, resolve},
    input::{ModId, ModRequest, Mods, Pack},
    lock::{BuildId, ReleaseTag},
    registry::{self, Https},
    selection::{self, Change, PackSource, Refresh, Target},
    transaction::{self, ServerFolder},
};
use sunrise_server_launch::{
    Install,
    config::{self, Config},
};

const GAME: &str = sunrise_platform::version::VERSION_ID;
const HELP: &str =
    "sunrise-server mods <add|remove|disable|enable|update|list|search|rollback> [--json]";

pub enum Action {
    Add(Vec<ModRequest>, Option<PackSource>),
    Remove(Vec<ModId>),
    Disable(Vec<ModId>),
    Enable(Vec<ModId>),
    Update(Vec<ModId>),
    List,
    Search(String),
    Rollback,
}

struct Command {
    action: Action,
    json: bool,
    check: bool,
    yes: bool,
}

impl Command {
    fn parse(args: &[String]) -> Result<Self, String> {
        let Some((verb, rest)) = args.split_first() else {
            return Err("choose a mods command".into());
        };
        let mut cmd = Self {
            action: Action::List,
            json: false,
            check: false,
            yes: false,
        };
        let mut operands = Vec::new();
        let mut pack = None;
        let mut args = rest.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--json" => cmd.json = true,
                "--check" if verb == "update" => cmd.check = true,
                "--yes" | "-y" if matches!(verb.as_str(), "update" | "rollback") => cmd.yes = true,
                "--pack" if verb == "add" && pack.is_none() => {
                    let raw = args
                        .next()
                        .ok_or("--pack needs a registry ID, .spack path or HTTPS URL")?;
                    pack = Some(PackSource::try_from(raw.clone())?);
                }
                option if option.starts_with('-') => {
                    return Err(format!("mods {verb}: unknown or repeated option {option}"));
                }
                _ => operands.push(arg.clone()),
            }
        }
        let ids = || {
            operands
                .iter()
                .cloned()
                .map(ModId::try_from)
                .collect::<Result<Vec<_>, _>>()
        };
        cmd.action = match verb.as_str() {
            "add" if pack.is_some() || !operands.is_empty() => Action::Add(
                operands
                    .into_iter()
                    .map(ModRequest::try_from)
                    .collect::<Result<_, _>>()?,
                pack,
            ),
            "remove" if !operands.is_empty() => Action::Remove(ids()?),
            "disable" if !operands.is_empty() => Action::Disable(ids()?),
            "enable" if !operands.is_empty() => Action::Enable(ids()?),
            "update" => Action::Update(ids()?),
            "list" if operands.is_empty() => Action::List,
            "search" if !operands.is_empty() => Action::Search(operands.join(" ")),
            "rollback" if operands.is_empty() => Action::Rollback,
            _ => return Err(format!("mods {verb}: wrong arguments")),
        };
        Ok(cmd)
    }
}

pub(super) fn index_location() -> String {
    std::env::var("SUNRISE_MOD_INDEX")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| sunrise_addresses::MOD_INDEX.to_owned())
}

pub(super) fn current_target(
    index: &Index,
    config: &Config,
    old: Option<&Lock>,
) -> Result<Target<'static>, String> {
    let selected = config
        .engine
        .path
        .is_empty()
        .then(Install::find)
        .flatten()
        .and_then(|install| install.current());
    if let Some(tag) = selected {
        let build = index
            .releases
            .iter()
            .flatten()
            .find(|release| release.tag == tag)
            .map(|release| &release.commit)
            .or_else(|| {
                let nightly = index.nightly_build.as_ref()?;
                let stem = format!("nightly-{}-", nightly.date);
                let short = tag.strip_prefix(&stem)?;
                (short.len() >= 7 && nightly.commit.starts_with(short)).then_some(&nightly.commit)
            });
        if let Some(build) = build {
            return Ok(Target {
                game: GAME,
                build: BuildId::try_from(build.clone())?,
                release: Some(ReleaseTag::try_from(tag)?),
                side: Side::Server,
            });
        }
        if let Some(old) = old
            && old
                .release
                .as_ref()
                .is_some_and(|release| release.to_string() == tag)
        {
            return Ok(Target {
                game: GAME,
                build: old.sunrise.clone(),
                release: old.release.clone(),
                side: Side::Server,
            });
        }
        return Err(format!(
            "the mod registry doesn't know the build for Sunrise {tag}\n  Choose a published build: sunrise-server update"
        ));
    }
    if let Some(old) = old {
        return Ok(Target {
            game: GAME,
            build: old.sunrise.clone(),
            release: old.release.clone(),
            side: Side::Server,
        });
    }
    Err("the installed Sunrise release is unknown\n  Choose a published build: sunrise-server update".into())
}

pub fn command(dir: &Path, args: &[String]) -> Result<(), String> {
    if args == ["--help"] || args == ["-h"] {
        println!(
            "{HELP}\n  add ID[@RANGE]… | --pack ID[@VERSION]|FILE.spack|URL\n  remove|disable|enable ID…\n  update [ID…] [--check] [--yes]\n  search TEXT…\n  list\n  rollback [--yes]"
        );
        return Ok(());
    }
    let cmd = Command::parse(args)
        .map_err(|error| format!("{error}\n  See usage: sunrise-server mods --help"))?;
    if !dir.join(config::FILE).is_file() {
        return Err("no server.toml here: run it in your server's folder, or add --dir FOLDER\n  Set one up: sunrise-server init --eula".into());
    }
    (|| match cmd.action {
        Action::List => {
            let _lease = transaction::read_lease(dir)?;
            let root = dir
                .canonicalize()
                .map_err(|e| format!("{}: {e}", dir.display()))?;
            let lock = read_optional_lock(&root)?
                .ok_or("no managed mods here\n  Find a mod: sunrise-server mods search mods")?;
            warn_takedowns(&root, &lock);
            write_list(&lock, io::stdout().lock(), cmd.json).map_err(|e| e.to_string())
        }
        Action::Search(query) => {
            let index = registry::refresh(dir, &index_location(), &Https::default())?;
            search(&index, &query, cmd.json)
        }
        action => {
            let folder = ServerFolder::open(dir)?;
            if matches!(action, Action::Rollback) && folder.recover()? {
                if cmd.json {
                    println!("{{\"recovered\":true}}");
                } else {
                    println!("Recovered the interrupted mod change.");
                }
                return Ok(());
            }
            let text = fs::read_to_string(folder.root().join(config::FILE))
                .map_err(|e| format!("{}: {e}", config::FILE))?;
            let cfg = Config::parse(&text)?;
            make_change(
                &folder, &text, None, &cfg, action, cmd.json, cmd.check, cmd.yes,
            )
        }
    })()
    .map_err(|error| with_next(error, "sunrise-server mods --help"))
}

fn with_next(error: String, command: &str) -> String {
    if error
        .lines()
        .last()
        .is_some_and(|line| line.starts_with("  ") && line.contains("sunrise-server "))
    {
        error
    } else {
        format!("{error}\n  Next: {command}")
    }
}

pub fn init(
    dir: &Path,
    cfg: &Config,
    input: Vec<ModRequest>,
    pack: Option<PackSource>,
    json: bool,
    yes: bool,
) -> Result<(), String> {
    let folder = ServerFolder::open(dir)?;
    let text = config::render(cfg);
    make_change(
        &folder,
        &text,
        Some(""),
        cfg,
        Action::Add(input, pack),
        json,
        false,
        yes,
    )
    .map_err(|error| with_next(error, "sunrise-server init --eula"))
}

fn read_optional_lock(root: &Path) -> Result<Option<Lock>, String> {
    if root
        .join(super::LOCK)
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        Lock::read(root).map(Some)
    } else {
        Ok(None)
    }
}

#[allow(clippy::too_many_arguments)]
fn make_change(
    folder: &ServerFolder,
    text: &str,
    expect: Option<&str>,
    cfg: &Config,
    action: Action,
    json: bool,
    check: bool,
    yes: bool,
) -> Result<(), String> {
    let root = folder.root();
    if [
        "sunrise-mods",
        "sunrise-mods.txt",
        "mods.lock",
        ".mods-previous",
        ".mods-transaction",
        ".mods-pack.spack",
    ]
    .iter()
    .any(|name| cfg.world.name.eq_ignore_ascii_case(name))
    {
        return Err("the world name is reserved for managed mod files; rename the world folder and set world.name before managing mods\n  Check the world setting: sunrise-server config".into());
    }
    let old = read_optional_lock(root)?;
    if matches!(action, Action::Rollback) {
        let prior = folder.previous_lock()?;
        if let Some(prior) = &prior {
            let path = root.join(transaction::PREVIOUS_DIR);
            check_files(prior, &path, &prior.input).map_err(|e| e.to_string())?;
            selection::check_enabled(&path, prior)?;
        }
        let changes = selection::changes(old.as_ref(), prior.as_ref());
        show_plan(&changes, json);
        if yes || confirm("Roll back?", false)? {
            folder.rollback(text)?;
            restored(json);
        } else if json {
            println!("{{\"changed\":false}}");
        }
        return Ok(());
    }
    let fetch = Https::default();
    let index = registry::refresh(root, &index_location(), &fetch).map_err(|e| {
        format!("{e}\n  Check the registry again: sunrise-server mods update --check")
    })?;
    let target = current_target(&index, cfg, old.as_ref())?;
    let mut input = cfg.mods.clone().unwrap_or_default();
    let mut source = input.pack.clone().map(PackSource::Exact);
    let mut pack_changed = false;
    let mut refresh = Refresh::None;
    let mut update = false;
    let targeted;
    match action {
        Action::Add(requests, pack) => {
            for request in requests {
                input.list.retain(|old| old.id != request.id);
                input.list.push(request);
            }
            if pack.is_some() {
                source = pack;
                pack_changed = true;
            }
        }
        Action::Remove(ids) => {
            change_ids(&index, &mut input, old.as_ref(), &ids, Edit::Remove)?;
        }
        Action::Disable(ids) => {
            change_ids(&index, &mut input, old.as_ref(), &ids, Edit::Disable)?;
        }
        Action::Enable(ids) => {
            change_ids(&index, &mut input, old.as_ref(), &ids, Edit::Enable)?;
        }
        Action::Update(ids) => {
            update = true;
            if let Some(Pack::Registry { id, .. }) = &input.pack
                && (ids.is_empty() || ids.contains(id))
            {
                source = Some(PackSource::Newest(id.clone()));
            }
            for id in &ids {
                let installed = old
                    .as_ref()
                    .is_some_and(|old| old.mods.iter().any(|row| row.id == *id));
                let pack =
                    matches!(&input.pack, Some(Pack::Registry { id: pack, .. }) if pack == id);
                if !installed && !pack {
                    return Err(format!(
                        "{id} isn't installed\n  Add it: sunrise-server mods add {id}"
                    ));
                }
            }
            offer_takedowns(&index, &mut input, old.as_ref(), &ids, check, yes, json)?;
            targeted = ids;
            refresh = if targeted.is_empty() {
                Refresh::All
            } else {
                Refresh::Only(&targeted)
            };
        }
        Action::List | Action::Search(_) | Action::Rollback => {
            unreachable!("read commands do not stage an install")
        }
    }
    let stage = folder.stage()?;
    let mut pack = None;
    if let Some(source) = source {
        let (pin, contents) =
            selection::stage_pack(root, &stage, &fetch, &index, GAME, source, !pack_changed)?;
        pack_changed |= Some(&pin) != input.pack.as_ref();
        input.pack = Some(pin);
        pack = Some(contents);
    }
    let plan = selection::plan(
        &index,
        &target,
        input,
        pack.as_ref().map(|p| &p.manifest),
        old.as_ref(),
        refresh,
        |_, _| false,
    )?;
    let changes = selection::changes(old.as_ref(), Some(&plan.lock));
    let modifies = !changes.is_empty()
        || old
            .as_ref()
            .is_none_or(|old| old.input != plan.lock.input || old.sunrise != plan.lock.sunrise);
    show_plan(&changes, json);
    if !plan.skipped_pack_members.is_empty() {
        eprintln!(
            "Skipping client-only pack members and their overrides: {}",
            plan.skipped_pack_members.join(", ")
        );
    }
    if check {
        if json {
            println!(
                "{}",
                serde_json::json!({"changed": modifies, "plan": changes})
            );
        } else if !modifies {
            println!("The mod selection is up to date.");
        }
        return Ok(());
    }
    let healthy = old.as_ref().is_some_and(|old| {
        check_files(old, root, &old.input).is_ok() && selection::check_enabled(root, old).is_ok()
    });
    if !modifies && healthy {
        if json {
            println!("{{\"changed\":false}}");
        } else {
            println!("The mod selection is up to date.");
        }
        return Ok(());
    }
    if update && !yes && !json && !confirm("Apply this mod update?", true)? {
        println!("No changes made.");
        return Ok(());
    }
    eprintln!("Verifying {} mod files…", plan.artifacts.len());
    selection::stage_content(
        root,
        &stage,
        &fetch,
        &index,
        old.as_ref(),
        &plan,
        pack_changed,
    )?;
    selection::stage_records(root, &stage, old.as_ref(), &plan.lock, text)?;
    folder.commit(stage, expect.unwrap_or(text))?;
    if json {
        println!(
            "{}",
            serde_json::json!({"changed": true, "plan": changes, "lock": plan.lock})
        );
    } else {
        println!(
            "Done. Restart to use it: sunrise-server restart\nUndo: sunrise-server mods rollback"
        );
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Edit {
    Remove,
    Disable,
    Enable,
}

fn change_ids(
    index: &Index,
    input: &mut Mods,
    old: Option<&Lock>,
    ids: &[ModId],
    edit: Edit,
) -> Result<(), String> {
    let old = old.ok_or("no managed mods here\n  Find a mod: sunrise-server mods search mods")?;
    let installed = old
        .mods
        .iter()
        .filter(|row| edit == Edit::Remove || row.on)
        .filter_map(|row| {
            let version = index
                .project(row.id.as_str())?
                .versions
                .iter()
                .find(|v| v.file.sha256 == row.sha256.as_str())?;
            Some(resolve::InstalledMod {
                id: row.id.to_string(),
                version: version.version.clone(),
                sha256: Some(version.file.sha256.clone()),
            })
        })
        .collect::<Vec<_>>();
    for id in ids {
        let row = old.mods.iter().find(|row| row.id == *id).ok_or_else(|| {
            format!("{id} isn't installed\n  Add it: sunrise-server mods add {id}")
        })?;
        if edit == Edit::Remove && row.reason == super::Reason::Pack {
            return Err(format!(
                "{id} comes with the pack\n  Turn it off: sunrise-server mods disable {id}"
            ));
        }
        if edit != Edit::Enable {
            let by = resolve::removal_dependents(index, &installed, id.as_str(), Side::Server)?;
            let remaining: Vec<_> = by
                .iter()
                .filter(|owner| !ids.iter().any(|target| target.as_str() == owner.id))
                .map(|owner| owner.id.as_str())
                .collect();
            if !remaining.is_empty() {
                let verb = if edit == Edit::Remove {
                    "remove"
                } else {
                    "disable"
                };
                return Err(format!(
                    "{} needs {id}\n  Change them together: sunrise-server mods {verb} {} {id}",
                    remaining.join(", "),
                    remaining.join(" ")
                ));
            }
        }
        match edit {
            Edit::Remove => {
                input.list.retain(|request| request.id != *id);
                input.off.retain(|off| off != id);
            }
            Edit::Disable if !input.off.contains(id) => input.off.push(id.clone()),
            Edit::Enable => input.off.retain(|off| off != id),
            Edit::Disable => {}
        }
    }
    Ok(())
}

fn offer_takedowns(
    index: &Index,
    input: &mut Mods,
    old: Option<&Lock>,
    ids: &[ModId],
    check: bool,
    yes: bool,
    json: bool,
) -> Result<(), String> {
    for row in old.into_iter().flat_map(|lock| &lock.mods) {
        if !ids.is_empty() && !ids.contains(&row.id) {
            continue;
        }
        let Some(revoked) = index.revoked(row.id.as_str(), row.sha256.as_str()) else {
            continue;
        };
        eprintln!(
            "{} {} was taken down ({}). Remove it: sunrise-server mods remove {}",
            row.id, row.version, revoked.reason, row.id
        );
        if !check
            && !yes
            && !json
            && io::stdin().is_terminal()
            && row.reason == super::Reason::Requested
            && confirm(&format!("Remove {}?", row.id), false)?
        {
            change_ids(
                index,
                input,
                old,
                std::slice::from_ref(&row.id),
                Edit::Remove,
            )?;
        }
    }
    Ok(())
}

fn confirm(question: &str, default: bool) -> Result<bool, String> {
    if !io::stdin().is_terminal() {
        return Ok(true);
    }
    eprint!("{question} [{}] ", if default { "Y/n" } else { "y/N" });
    io::stderr().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let answer = line.trim();
    Ok((answer.is_empty() && default)
        || answer.eq_ignore_ascii_case("y")
        || answer.eq_ignore_ascii_case("yes"))
}

fn show_plan(changes: &[Change<'_>], json: bool) {
    let out = |line: &str| {
        if json {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    };
    if changes.is_empty() {
        out("No version or on/off changes.");
    }
    for change in changes {
        let status = if change.on == Some(false) {
            " (off)"
        } else {
            ""
        };
        out(&match (change.from, change.to) {
            (None, Some(to)) => format!("  {}: install {to}{status}", change.id),
            (Some(from), None) => format!("  {}: remove {from}", change.id),
            (Some(from), Some(to)) if from != to => {
                format!("  {}: {from} → {to}{status}", change.id)
            }
            (_, Some(to)) => format!(
                "  {}: {to} ({})",
                change.id,
                if change.on == Some(true) { "on" } else { "off" }
            ),
            _ => continue,
        });
    }
}

fn restored(json: bool) {
    if json {
        println!("{{\"changed\":true}}");
    } else {
        println!("Restored the previous mod selection. Restart to use it: sunrise-server restart");
    }
}

fn search(index: &Index, query: &str, json: bool) -> Result<(), String> {
    let query = query.to_lowercase();
    let results: Vec<_> = index
        .projects
        .iter()
        .filter(|project| {
            matches!(project.kind, Kind::Mod | Kind::NativeMod | Kind::Modpack)
                && [&project.id, &project.name, &project.summary]
                    .iter()
                    .any(|field| field.to_lowercase().contains(&query))
        })
        .filter_map(|project| {
            index
                .installable(project, GAME)
                .map(|version| (project, version))
        })
        .collect();
    if json {
        let output: Vec<_> = results.iter().map(|(project, version)| serde_json::json!({
            "id": project.id, "version": version.version, "kind": project.kind, "side": version.side, "authors": project.authors, "summary": project.summary,
        })).collect();
        println!("{}", serde_json::json!({"results": output}));
    } else {
        for (project, version) in &results {
            let side = if version.side == Some(Side::Client) {
                " (client-only: not for servers)"
            } else {
                ""
            };
            println!(
                "{}  {}  {}{side}  {}",
                project.id,
                version.version,
                project.kind.name(),
                project.summary
            );
        }
        if let Some((project, _)) = results
            .iter()
            .find(|(p, v)| v.side != Some(Side::Client) && p.kind != Kind::NativeMod)
        {
            let flag = if project.kind == Kind::Modpack {
                "--pack "
            } else {
                ""
            };
            println!("Add: sunrise-server mods add {flag}{}", project.id);
        } else if results.is_empty() {
            println!("No matching reviewed mods.");
        }
    }
    Ok(())
}

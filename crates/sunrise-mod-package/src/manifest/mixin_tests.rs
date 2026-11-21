use super::super::Manifest;
use super::*;

const HEADER: &str =
    "id='sample'\nname='Sample'\nversion='1.0.0'\napi='0.2'\ncapabilities=['registries']\n";
const ROW: &str = "[[native.mixin]]\nid='autosave'\ntarget='game::Game::next_autosave'\nkind='MODIFY_EXPRESSION_VALUE'\nhelper='src/longer.rs'\n";

fn native() -> String {
    format!(
        "[native]\ngame='26.3'\nsunrise='{0}'\nmixins='{0}'\n",
        "a".repeat(40)
    )
}

fn parse(code: &str, rows: &str) -> Result<Manifest, String> {
    Manifest::parse(&format!("{HEADER}{code}{rows}"))
}

#[test]
fn top_level_mixin_is_unknown() {
    assert!(
        Manifest::parse(&format!("{HEADER}[[mixin]]\nid='a'\n"))
            .unwrap_err()
            .contains("unknown field `mixin`")
    );
}

#[test]
fn the_tools_keys_pass_through_and_the_packages_are_checked() {
    let row = format!(
        "{ROW}selector='self.interval'\nordinal=1\nrequire=1\nfingerprint='abc123'\ncancellable=true\npriority=5\nbindings=[{{name='n',ty='u32',value='n'}}]\nslice={{from={{selector='a',ordinal=0,require=1}}}}\nvariable={{name='n',ordinal=0,at='STORE'}}\n"
    );
    let manifest = parse(&native(), &row).unwrap();
    let mixin = &manifest.native.as_ref().unwrap().mixin[0];
    assert_eq!(mixin.kind, Kind::ModifyExpressionValue);
    // What the tool reads back is the row as written.
    let forwarded: toml::Table = toml::from_str(&toml::to_string(mixin).unwrap()).unwrap();
    let written: toml::Table = toml::from_str(&row).unwrap();
    assert_eq!(
        toml::Value::Table(forwarded),
        written["native"]["mixin"].as_array().unwrap()[0]
    );
    for (old, new, field) in [
        ("target='game::Game::next_autosave'", "target=' '", "target"),
        ("helper='src/longer.rs'", "helper='../longer.rs'", "helper"),
        ("helper='src/longer.rs'", "helper='/etc/passwd'", "helper"),
        (
            "helper='src/longer.rs'",
            "helper='src/longer.toml'",
            "helper",
        ),
        ("helper='src/longer.rs'", "helper='longer.rs'", "helper"),
        ("kind=", "hook='tick'\nkind=", "hook"),
        ("kind=", "hook_version=1\nkind=", "hook"),
    ] {
        let error = parse(&native(), &ROW.replace(old, new)).unwrap_err();
        assert!(
            error.contains(&format!(
                "E_MIXIN_INVALID: sample:autosave: invalid {field};"
            )),
            "{error}"
        );
    }
    for missing in [
        "id='autosave'\n",
        "target='game::Game::next_autosave'\n",
        "helper='src/longer.rs'\n",
    ] {
        assert!(
            parse(&native(), &ROW.replace(missing, "")).is_err(),
            "{missing}"
        );
    }
}

#[test]
fn a_mixin_never_targets_another_mods_function() {
    let row = ROW.replace("game::Game::next_autosave", "chem_lib::transfer");
    let dependency = "[dependencies]\nchem_lib='^1.0'\n";
    // Without the dependency the path is a game path for the tool to resolve.
    assert!(parse(&native(), &row).is_ok());
    let error = parse(&format!("{dependency}{}", native()), &row).unwrap_err();
    assert!(
        error.ends_with(
            "E_MIXIN_INVALID: sample:autosave: invalid target; a mixin targets a game function"
        ),
        "{error}"
    );
}

#[test]
fn hostile_shapes_are_refused_without_panicking() {
    let shapes = [
        // a kind that is no string, or an empty one
        ROW.replace("kind='MODIFY_EXPRESSION_VALUE'", "kind=3"),
        ROW.replace("kind='MODIFY_EXPRESSION_VALUE'", "kind=''"),
        // tables where arrays of tables belong, and the reverse
        ROW.replace("[[native.mixin]]", "[native.mixin]"),
        "[[native]]\ngame='26.3'\n".into(),
        "[[native.bridge]]\nid=7\n".into(),
        "[[native.state]]\nid=['a']\n".into(),
        // keys of the wrong type, blank ids, and ids that are not ASCII
        ROW.replace("helper='src/longer.rs'", "helper=1"),
        ROW.replace("helper='src/longer.rs'", "helper={path='src/longer.rs'}"),
        ROW.replace("target='game::Game::next_autosave'", "target=[]"),
        ROW.replace("id='autosave'", "id=' '"),
        ROW.replace("id='autosave'", "id=''"),
        "[[native.bridge]]\nid=' '\n".into(),
        "[[native.state]]\nid='\t'\n".into(),
        ROW.replace("helper='src/longer.rs'", "helper='src/\u{0}.rs\\..'"),
        // a duplicate key
        format!("{ROW}kind='HEAD'\n"),
    ];
    for shape in &shapes {
        for code in [native(), String::new()] {
            assert!(parse(&code, shape).is_err(), "{shape}{code}");
        }
    }
    // Unicode in an id is the tool's to judge, and is carried as written.
    let manifest = parse(&native(), &ROW.replace("id='autosave'", "id='épée-✓'")).unwrap();
    assert_eq!(manifest.native.unwrap().mixin[0].id, "épée-✓");
}

#[test]
fn mixins_bridges_states_modules_share_one_id_space() {
    let tables = [
        ROW.to_owned(),
        "[[native.module]]\nid='autosave'\nfile='game.rs'\nname='runtime'\nsource='src/runtime.rs'\n".into(),
        "[[native.bridge]]\nid='autosave'\nkind='GET'\ntarget='game::Game'\nmember='world'\n".into(),
        "[[native.state]]\nid='autosave'\ntarget='game::Game'\nty='u32'\ninitial='0'\nconstructor=[]\n".into(),
    ];
    for (a, first) in tables.iter().enumerate() {
        // Top-level rows precede `[native]`; its own tables follow it.
        let text = |second: &str| {
            let (top, nested): (Vec<&str>, Vec<&str>) = [first.as_str(), second]
                .into_iter()
                .partition(|table| !table.starts_with("[[native."));
            format!("{HEADER}{}{}{}", top.concat(), native(), nested.concat())
        };
        assert!(Manifest::parse(&text("")).is_ok(), "{first}");
        for second in &tables[a + 1..] {
            let error = Manifest::parse(&text(second)).unwrap_err();
            assert!(error.contains("duplicate"), "{first}{second}: {error}");
            assert!(
                Manifest::parse(&text(&second.replace("id='autosave'", "id='other'"))).is_ok(),
                "{first}{second}"
            );
        }
    }
    let manifest = Manifest::parse(&format!(
        "{HEADER}{}{}{}",
        native(),
        tables[2],
        tables[3].replace("id='autosave'", "id='count'")
    ))
    .unwrap();
    let native = manifest.native.unwrap();
    assert_eq!(native.bridge[0].fields["member"].as_str(), Some("world"));
    assert_eq!(native.state[0].fields["ty"].as_str(), Some("u32"));
}

/// A bridge's id names its accessor, so it is refused when it can't, naming the mod and row.
#[test]
fn a_bridge_id_that_cannot_name_an_accessor_is_refused() {
    let bridge = |id: &str| {
        format!("[[native.bridge]]\nid='{id}'\nkind='GET'\ntarget='game::Game'\nmember='world'\n")
    };
    assert!(parse(&native(), &bridge("ticks-current_2")).is_ok());
    let error = parse(&native(), &bridge("ticks.current")).unwrap_err();
    assert!(
        error.contains("E_MIXIN_INVALID: sample:ticks.current: invalid id; a bridge id is ASCII letters, digits, `_` and `-`"),
        "{error}"
    );
}

#[test]
fn two_bridge_ids_with_one_accessor_name_are_refused() {
    let bridge = |id: &str| {
        format!("[[native.bridge]]\nid='{id}'\nkind='GET'\ntarget='game::Game'\nmember='world'\n")
    };
    let error = parse(
        &native(),
        &format!("{}{}", bridge("ticks-a"), bridge("Ticks_A")),
    )
    .unwrap_err();
    assert!(
        error.contains("E_MIXIN_INVALID: sample:Ticks_A: invalid id; it gives the same accessor name as bridge `ticks-a`"),
        "{error}"
    );
}

use super::super::Manifest;

const HEADER: &str =
    "id='sample'\nname='Sample'\nversion='1.0.0'\napi='0.2'\ncapabilities=['registries']\n";
const ROW: &str =
    "[[native.loop]]\nid='slow'\nloop='block_entities.tick'\nhelper='src/slow.rs'\nwith='Slow'\n";

fn native() -> String {
    format!(
        "[native]\ngame='26.3'\nsunrise='{0}'\nmixins='{0}'\n",
        "a".repeat(40)
    )
}

#[test]
fn a_row_reads_with_its_default_priority() {
    let manifest = Manifest::parse(&format!("{HEADER}{}{ROW}", native())).unwrap();
    let row = &manifest.native.unwrap().r#loop[0];
    assert_eq!(row.name, "block_entities.tick");
    assert_eq!(row.with, "Slow");
    assert_eq!(row.priority, 1000);
    let manifest = Manifest::parse(&format!("{HEADER}{}{ROW}priority=-5\n", native())).unwrap();
    assert_eq!(manifest.native.unwrap().r#loop[0].priority, -5);
}

#[test]
fn bad_rows_are_refused_without_panicking() {
    for (old, new, field) in [
        ("with='Slow'", "with='crate::Slow'", Some("with")),
        ("with='Slow'", "with='a b'", Some("with")),
        ("with='Slow'", "with=''", Some("with")),
        (
            "helper='src/slow.rs'",
            "helper='../slow.rs'",
            Some("helper"),
        ),
        (
            "helper='src/slow.rs'",
            "helper='src/slow.toml'",
            Some("helper"),
        ),
        ("loop='block_entities.tick'", "loop=' '", Some("loop")),
        ("with='Slow'", "with=3", None),
        ("with='Slow'", "with='Slow'\nextra=1", None),
        ("helper='src/slow.rs'\n", "", None),
        ("[[native.loop]]", "[native.loop]", None),
        ("loop='block_entities.tick'\n", "", None),
    ] {
        let error =
            Manifest::parse(&format!("{HEADER}{}{}", native(), ROW.replace(old, new))).unwrap_err();
        if let Some(field) = field {
            assert!(
                error.contains(&format!("E_LOOP_INVALID: sample:slow: invalid {field};")),
                "{error}"
            );
        }
    }
}

#[test]
fn loop_rows_share_the_native_id_space() {
    let mixin =
        "[[native.mixin]]\nid='slow'\ntarget='game::Game::f'\nkind='HEAD'\nhelper='src/h.rs'\n";
    let error = Manifest::parse(&format!("{HEADER}{}{ROW}{mixin}", native())).unwrap_err();
    assert!(error.contains("duplicate"), "{error}");
    let error = Manifest::parse(&format!("{HEADER}{}{ROW}{ROW}", native())).unwrap_err();
    assert!(error.contains("duplicate"), "{error}");
}

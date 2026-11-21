use super::*;
use sunrise_connected::Method;

/// The folder, removed when the test ends.
struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const DIR: &str = "assets/demo/sunrise/connected";

/// A resource folder holding the `files` (path, text), the textures `demo:block/casing` and
/// `demo:block/casing_sheet`, and the block tag `demo:casings`.
fn folder(name: &str, files: &[(&str, &str)]) -> (Scratch, ResourceSource) {
    let root = std::env::temp_dir().join(format!("sunrise-ct-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let all = [
        ("assets/demo/textures/block/casing.png", ""),
        ("assets/demo/textures/block/casing_sheet.png", ""),
        (
            "data/demo/tags/block/casings.json",
            r#"{"values":["demo:brass_casing","demo:copper_casing"]}"#,
        ),
    ];
    for (path, text) in all.iter().chain(files) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    (Scratch(root.clone()), ResourceSource::directory(root))
}

fn parse_in(root: &ResourceSource, text: &str) -> Result<Rule, String> {
    parse(
        &format!("{DIR}/casing.json"),
        text.as_bytes(),
        root,
        &Tags::new(root),
        &mut Tables::new(),
    )
}

const ROW: &str = r#"{"tests":["left","right"],"grid":[4,1],"tiles":[3,2,0,1]}"#;

#[test]
fn a_file_is_a_table_rule() {
    let (_scratch, root) = folder(
        "good",
        &[("assets/demo/sunrise/connected_tables/row.json", ROW)],
    );
    let rule = parse_in(
            &root,
            r##"{"method":"table","blocks":["demo:casing",{"block":"demo:log","state":{"axis":["x","z"]}}],
                "sprites":["demo:block/casing"],"faces":["sides"],"connect":{"tag":"#demo:casings"},
                "orient":"axis","sheet":"demo:block/casing_sheet","table":"row"}"##,
        )
        .unwrap();
    assert_eq!(rule.method, Method::Table);
    assert_eq!(rule.faces, faces::SIDES);
    assert_eq!(rule.orient, Orient::StateAxis);
    assert_eq!(rule.match_tiles, ["assets/demo/textures/block/casing.png"]);
    assert!(rule.prioritize);
    assert_eq!(rule.connect, Connect::Blocks);
    let ids: Vec<_> = rule.connect_blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["demo:brass_casing", "demo:copper_casing"]);
    assert!(rule.matches_block("demo:log", |_| Some("z")));
    assert!(!rule.matches_block("demo:log", |_| Some("y")));
    let sheet = rule.table.unwrap();
    assert_eq!(sheet.sprite, "demo:block/casing_sheet");
    assert_eq!(sheet.table.grid(), (4, 1));
    // The inline form reads the same.
    let inline = parse_in(
            &root,
            &format!(r#"{{"method":"table","tag":"demo:casings","sheet":"demo:block/casing_sheet","table":{ROW}}}"#),
        )
        .unwrap();
    assert_eq!(inline.table.unwrap().table, sheet.table);
    assert_eq!(inline.match_blocks.len(), 2);
    let pillar = parse_in(
            &root,
            &format!(r#"{{"method":"table","blocks":["demo:log"],"connect":{{"same":["axis"]}},"sheet":"demo:block/casing_sheet","table":{ROW}}}"#),
        )
        .unwrap();
    assert_eq!(pillar.connect, Connect::Properties);
    assert_eq!(pillar.connect_properties, ["axis"]);
}

#[test]
fn a_file_can_join_by_height() {
    let (_scratch, root) = folder(
        "height",
        &[("assets/demo/sunrise/connected_tables/row.json", ROW)],
    );
    let file = |connect: &str| {
        parse_in(
            &root,
            &format!(
                r#"{{"method":"table","blocks":["demo:roof"],"connect":{connect},"sheet":"demo:block/casing_sheet","table":"row"}}"#
            ),
        )
    };
    let rule =
        file(r#"{"height":{"same":true,"above":true,"side":true,"of":["demo:shingles","roof"]}}"#)
            .unwrap();
    assert_eq!(
        rule.connect,
        Connect::Height(HeightJoin {
            same: true,
            above: true,
            side: true,
            ..Default::default()
        })
    );
    let of: Vec<_> = rule.connect_blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(of, ["demo:shingles", "demo:roof"]);
    // A rule with no clause, a `side` with nothing to look for, an `of` with nothing to
    // compare, an unknown clause and a second way to join are all refused.
    for connect in [
        r#"{"height":{}}"#,
        r#"{"height":{"same":true,"side":true}}"#,
        r#"{"height":{"higher":true,"of":["demo:roof"]}}"#,
        r#"{"height":{"heigher":true}}"#,
        r#"{"height":{"same":true},"same":["axis"]}"#,
    ] {
        assert!(file(connect).is_err(), "{connect}");
    }
    // An unknown clause is named.
    let err = file(r#"{"height":{"heigher":true}}"#).unwrap_err();
    assert!(err.contains("heigher"), "{err}");
}

#[test]
fn a_file_that_cannot_load_is_refused_by_name() {
    let (_scratch, root) = folder(
        "bad",
        &[
            ("assets/demo/sunrise/connected_tables/row.json", ROW),
            (
                "assets/demo/sunrise/connected_tables/short.json",
                r#"{"tests":["left","right"],"grid":[4,1],"tiles":[0,1,2]}"#,
            ),
        ],
    );
    // The fields in `extra` replace the base file's: a repeated key is refused by serde.
    let file = |extra: &str| {
        let mut json: serde_json::Value = serde_json::from_str(
                r#"{"method":"table","blocks":["demo:casing"],"sheet":"demo:block/casing_sheet","table":"row"}"#,
            )
            .unwrap();
        let replaced: serde_json::Value =
            serde_json::from_str(&format!("{{{}}}", extra.trim_start_matches(','))).unwrap();
        json.as_object_mut()
            .unwrap()
            .extend(replaced.as_object().unwrap().clone());
        parse_in(&root, &json.to_string())
    };
    assert!(file("").is_ok());
    let err = file(r#","sheet":"demo:block/missing""#).unwrap_err();
    assert!(err.contains("demo:block/missing"), "{err}");
    let err = file(r#","sprites":["demo:block/gone"]"#).unwrap_err();
    assert!(err.contains("demo:block/gone"), "{err}");
    let err = file(r#","table":"short""#).unwrap_err();
    assert!(
        err.contains("demo:short") && err.contains("3 tiles for 2 tests"),
        "{err}"
    );
    let err = file(r#","table":"absent""#).unwrap_err();
    assert!(err.contains("demo:absent"), "{err}");
    let err = file(r#","connect":{"tag":"demo:nothing"}"#).unwrap_err();
    assert!(err.contains("unknown block tag demo:nothing"), "{err}");
    let err = parse_in(&root, r#"{"method":"table","tag":"demo:nothing","sheet":"demo:block/casing_sheet","table":"row"}"#).unwrap_err();
    assert!(err.contains("unknown block tag demo:nothing"), "{err}");
    let err = file(r#","faces":["front"]"#).unwrap_err();
    assert!(err.contains("front"), "{err}");
    let err = parse_in(
        &root,
        r#"{"method":"table","sheet":"demo:block/casing_sheet","table":"row"}"#,
    )
    .unwrap_err();
    assert!(err.contains("names no block"), "{err}");
    let err = file(r#","colour":1"#).unwrap_err();
    assert!(err.contains("colour"), "{err}");
    // Combined or misspelt fields of a variant fail too, never dropped.
    let err = file(r#","connect":{"tag":"demo:casings","same":["axis"]}"#).unwrap_err();
    assert!(err.contains("one of tag, same and height"), "{err}");
    let err = file(r#","connect":{"same":["axis"],"sames":[]}"#).unwrap_err();
    assert!(err.contains("sames"), "{err}");
    let err = file(r#","blocks":[{"block":"demo:log","state":{},"stat":{}}]"#).unwrap_err();
    assert!(err.contains("stat"), "{err}");
}

#[test]
fn only_the_valid_files_of_a_folder_load() {
    let (_scratch, root) = folder(
        "read",
        &[
            ("assets/demo/sunrise/connected_tables/row.json", ROW),
            (
                "assets/demo/sunrise/connected/good.json",
                r#"{"method":"table","blocks":["demo:casing"],"sheet":"demo:block/casing_sheet","table":"row"}"#,
            ),
            ("assets/demo/sunrise/connected/broken.json", "{"),
        ],
    );
    let rules = read(&root);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].path, "assets/demo/sunrise/connected/good.json");
    // Nothing declared: nothing read (and the tag files are not even opened).
    assert!(read(&folder("none", &[]).1).is_empty());
}

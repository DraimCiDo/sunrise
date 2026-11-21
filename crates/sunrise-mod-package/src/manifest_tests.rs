/// A short version with a pre-release or build part is padded before it, not after it.
#[test]
fn a_short_version_with_a_pre_release_is_padded_in_its_core() {
    for (short, full) in [
        ("1", "1.0.0"),
        ("1.2", "1.2.0"),
        ("1.2.3", "1.2.3"),
        ("1.2-beta", "1.2.0-beta"),
        ("1-rc.1", "1.0.0-rc.1"),
        ("1.2-rc.1", "1.2.0-rc.1"),
        ("1.2.3-rc.1", "1.2.3-rc.1"),
        ("1.2+build.5", "1.2.0+build.5"),
        ("1.0-SNAPSHOT", "1.0.0-SNAPSHOT"),
    ] {
        assert_eq!(
            parse_version(short).unwrap(),
            semver::Version::parse(full).unwrap(),
            "{short}"
        );
    }
    assert!(parse_version("").is_err());
    assert!(parse_version("one.two").is_err());
}

use super::package::check_file;
use super::*;
use mod_surface::uses::USES_FILE;
use std::io::Read;

#[test]
fn a_version_that_is_not_semver_refuses_the_manifest_when_read() {
    let with = |v: &str| {
        Manifest::parse(&EXAMPLE.replace("version = \"0.1.0\"", &format!("version = '{v}'")))
    };
    let error = with("v1 beta!").unwrap_err();
    assert!(
            error.contains(
                "version \"v1 beta!\" is not a semver version (MAJOR.MINOR.PATCH, e.g. 3.6.30+refabricated)"
            ),
            "{error}"
        );
    assert!(with("").is_err());
}

#[test]
fn an_omitted_minor_and_patch_are_padded() {
    let with = |v: &str| {
        Manifest::parse(&EXAMPLE.replace("version = \"0.1.0\"", &format!("version = '{v}'")))
            .unwrap()
            .version
            .to_string()
    };
    assert_eq!(with("1"), "1.0.0");
    assert_eq!(with("1.2"), "1.2.0");
    assert_eq!(with("3.6.30+refabricated"), "3.6.30+refabricated");
}

#[test]
fn windows_device_mod_ids_are_refused() {
    for id in ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9"] {
        assert!(
            Manifest::parse(&EXAMPLE.replace("ruby", id))
                .unwrap_err()
                .contains("reserved Windows device name"),
            "{id}"
        );
    }
    for id in ["console", "com0", "com10", "lpt0", "con_tool"] {
        assert!(
            Manifest::parse(&EXAMPLE.replace("ruby", id)).is_ok(),
            "{id}"
        );
    }
}

const EXAMPLE: &str = r#"
id = "ruby"
name = "Ruby"
version = "0.1.0"
api = "0.2"
capabilities = ["registries"]
"#;

#[test]
fn api_capabilities_and_mixins_are_optional_and_an_explicit_other_value_is_refused() {
    let bare = "id = \"ruby\"\nname = \"Ruby\"\nversion = \"0.1.0\"\n";
    let manifest = Manifest::parse(bare).unwrap();
    assert_eq!(manifest.api, API);
    assert_eq!(manifest.capabilities, [Capability::Registries]);
    let error = Manifest::parse(&format!("{bare}capabilities = []\n")).unwrap_err();
    assert!(
        error.contains("[]") && error.contains("[\"registries\"]"),
        "{error}"
    );
    let native = format!(
        "{bare}[native]\ngame = \"26.3\"\nsunrise = \"{}\"\n",
        "a".repeat(40)
    );
    let native = Manifest::parse(&native).unwrap().native.unwrap();
    assert_eq!(native.mixins_or("sdk"), "sdk");
    let pinned = Native {
        mixins: Some("b".repeat(40)),
        ..native
    };
    assert_eq!(pinned.mixins_or("sdk"), "b".repeat(40));
}

#[test]
fn a_missing_or_misspelled_field_is_named_with_the_line_to_write() {
    let missing = Manifest::parse(&EXAMPLE.replace("version = \"0.1.0\"\n", "")).unwrap_err();
    assert!(missing.contains("version = \"0.1.0\""), "{missing}");
    let typo = Manifest::parse(&EXAMPLE.replace("version =", "vrsion =")).unwrap_err();
    assert!(typo.contains("unknown field `vrsion`"), "{typo}");
    assert!(typo.contains("did you mean `version`?"), "{typo}");
    let far = Manifest::parse(&format!("{EXAMPLE}zzzzzzzz = 1\n")).unwrap_err();
    assert!(!far.contains("did you mean"), "{far}");
}

#[test]
fn unknown_top_level_array_of_tables_is_refused() {
    let error = Manifest::parse(&format!(
        "{EXAMPLE}\n[[inject]]\nhook = 'player_logged_in'\n"
    ))
    .unwrap_err();
    assert!(error.contains("unknown field `inject`"), "{error}");
}

#[test]
fn refuses_bad_ids_and_retired_manifests() {
    let bad_id = EXAMPLE.replace("ruby", "Ruby!");
    assert!(Manifest::parse(&bad_id).unwrap_err().contains("is not"));
    for api in ["0.1", "0.3", "0.4"] {
        let text = EXAMPLE.replace("api = \"0.2\"", &format!("api = \"{api}\""));
        assert!(
            Manifest::parse(&text)
                .unwrap_err()
                .contains("unsupported content API")
        );
    }
    for caps in ["[]", "[\"hud\"]", "[\"registries\", \"registries\"]"] {
        let text = EXAMPLE.replace("[\"registries\"]", caps);
        assert!(Manifest::parse(&text).is_err());
    }
    for field in ["network = true", "experimental = []", "side = \"local\""] {
        assert!(Manifest::parse(&format!("{EXAMPLE}{field}\n")).is_err());
    }
    assert_eq!(Manifest::parse(EXAMPLE).unwrap().side, Side::Client);
    assert_eq!(
        Manifest::parse(&format!("{EXAMPLE}side = \"both\"\n"))
            .unwrap()
            .side,
        Side::Both
    );
    let server = Manifest::parse(&format!("{EXAMPLE}side = \"server\"\n")).unwrap();
    assert_eq!(server.side, Side::Server);
    let id = &server.id;
    assert_eq!(
        server.server_only_declaration(
            [
                ("block", "minecraft:stone"),
                ("item", &format!("{id}:marker")[..])
            ]
            .into_iter()
        ),
        Some(format!(
            "{id}: a side = \"server\" mod declares the item {id}:marker, which every client \
                 must have; declare side = \"both\""
        ))
    );
    assert_eq!(
        server.server_only_declaration([("block", "minecraft:stone")].into_iter()),
        None
    );
}

#[test]
fn native_manifest_requires_exact_pins_and_known_fields() {
    let native = format!(
        "[native]\ngame = '26.3'\nsunrise = '{}'\nmixins = '{}'\n",
        "a".repeat(40),
        "b".repeat(40)
    );
    Manifest::parse(&format!("{EXAMPLE}{native}")).unwrap();
    assert!(
        Manifest::parse(&format!(
            "{EXAMPLE}{}",
            native.replace(&"a".repeat(40), "main")
        ))
        .is_err()
    );
    for unsupported in ["unexpected = true", "[[native.post_build]]\nbin = 'bake'"] {
        assert!(Manifest::parse(&format!("{EXAMPLE}{native}{unsupported}\n")).is_err());
    }
}

#[test]
fn native_unknown_fields_are_refused_at_the_owning_boundary() {
    let pin = "a".repeat(40);
    let base = format!("{EXAMPLE}[native]\ngame='26.3'\nsunrise='{pin}'\nmixins='{pin}'\n");
    let mut accepted = Vec::new();
    for field in [
        "injection=[]",
        "[[native.injection]]\nid='tick'\ntarget='game::tick'\nkind='HEAD'\nraw='tick'",
        "internals=[]",
        "[[native.internals]]\npath='Game'\nhash='h'",
        "unexpected=true",
    ] {
        let text = format!("{base}{field}\n");
        match Manifest::parse(&text) {
            Ok(_) => accepted.push(field),
            Err(error) => assert!(error.contains("unknown field"), "{error}"),
        }
        // The same owning serde boundary applies outside Manifest::parse.
        if toml::from_str::<Native>(text.split("[native]\n").nth(1).unwrap()).is_ok() {
            accepted.push(field);
        }
    }
    assert!(
        accepted.is_empty(),
        "accepted unknown native fields: {accepted:?}"
    );
}

#[test]
fn internal_kinds_distinguish_members_and_round_trip() {
    let base = format!(
        "{EXAMPLE}[native]\ngame='26.3'\nsunrise='{}'\nmixins='{}'\n",
        "a".repeat(40),
        "b".repeat(40)
    );
    let declaration = "internals=[{path='Round::listening',kind='field',hash='1234'}, {path='Round::listening',kind='method',hash='5678'}]\n";
    let manifest =
        Manifest::parse(&base.replace("[native]", &format!("{declaration}[native]"))).unwrap();
    assert_eq!(manifest.internals[1].kind.as_deref(), Some("method"));
    let row = toml::to_string(&manifest.internals[1]).unwrap();
    assert_eq!(
        toml::from_str::<Internal>(&row).unwrap(),
        manifest.internals[1]
    );
    let native = manifest.native.unwrap();
    let text = toml::to_string(&native).unwrap();
    assert_eq!(toml::from_str::<Native>(&text).unwrap(), native);
    for kind in ["field", ""] {
        let invalid = declaration.replace("kind='method'", &format!("kind='{kind}'"));
        assert!(Manifest::parse(&base.replace("[native]", &format!("{invalid}[native]"))).is_err());
    }
    let untyped = Internal {
        path: "Game".into(),
        kind: None,
        hash: Some("1234".into()),
    };
    assert!(!toml::to_string(&untyped).unwrap().contains("kind"));
}

#[test]
fn native_modules_are_validated() {
    let native = format!(
        "[native]\ngame = '26.3'\nsunrise = '{}'\nmixins = '{}'\n",
        "a".repeat(40),
        "b".repeat(40)
    );
    let module = "[[native.module]]\nid = 'runtime'\nfile = 'crates/sunrise-client/src/game.rs'\nname = 'runtime'\nsource = 'src/runtime.rs'\n";
    let manifest = Manifest::parse(&format!("{EXAMPLE}{native}{module}")).unwrap();
    let module = &manifest.native.as_ref().unwrap().module[0];
    assert_eq!(module.name, "runtime");
    assert_eq!(module.visibility, None);
    // Support modules still forward whole to horizon-mixins.
    let forwarded = toml::to_string(manifest.native.as_ref().unwrap()).unwrap();
    assert!(forwarded.contains("[[module]]"));
    assert!(forwarded.contains("id = \"runtime\""));
    let valid = "[[native.module]]\nid = 'runtime'\nfile = 'crates/sunrise-client/src/game.rs'\nname = 'runtime'\nsource = 'src/runtime.rs'\n";
    for bad in [
        // escapes the package's src/
        valid.replace("source = 'src/runtime.rs'", "source = '../escape.rs'"),
        // unknown field
        format!("{valid}order = 1\n"),
        // duplicate module id
        format!("{valid}{valid}"),
        format!(
            "{valid}[[native.module]]\nid='runtime'\nfile='game.rs'\nname='other'\nsource='src/other.rs'\n"
        ),
        // not an identifier
        valid.replace("name = 'runtime'", "name = 'run time'"),
        // file must stay a plain path
        valid.replace(
            "file = 'crates/sunrise-client/src/game.rs'",
            "file = '/abs.rs'",
        ),
        // horizon-mixins accepts exactly pub and pub(crate)
        valid.to_owned() + "visibility = 'public'\n",
    ] {
        assert!(
            Manifest::parse(&format!("{EXAMPLE}{native}{bad}")).is_err(),
            "accepted: {bad}"
        );
    }
    let public = valid.to_owned() + "visibility = 'pub(crate)'\n";
    assert!(Manifest::parse(&format!("{EXAMPLE}{native}{public}")).is_ok());
}

#[test]
fn properties_are_typed_and_bounded() {
    let properties = |rows: &str| Manifest::parse(&format!("{EXAMPLE}[properties]\n{rows}"));
    let manifest = properties("tier = 3\nratio = 0.5\nlabel = 'x'\nfast = true\n").unwrap();
    assert_eq!(manifest.properties["tier"], Property::Integer(3));
    assert_eq!(manifest.properties["ratio"], Property::Float(0.5));
    assert_eq!(manifest.properties["label"], Property::String("x".into()));
    assert_eq!(manifest.properties["fast"], Property::Boolean(true));
    let many: String = (0..65).map(|i| format!("k{i} = {i}\n")).collect();
    let error = properties(&many).unwrap_err();
    assert!(error.contains("at most 64 keys"), "{error}");
    assert!(properties(&format!("text = '{}'\n", "a".repeat(8 << 10))).is_err());
    assert!(properties("list = [1]\n").is_err());
}

#[test]
fn dependencies_are_typed_for_content_and_code_mods() {
    let deps = "[dependencies]\ncreate = '^6.0'\naddon = { version = '^1.2', optional = true }\n";
    let content = Manifest::parse(&format!("{EXAMPLE}{deps}")).unwrap();
    assert_eq!(content.dependencies.len(), 2);
    let native = format!(
        "[native]\ngame = '26.3'\nsunrise = '{}'\nmixins = '{}'\n",
        "a".repeat(40),
        "b".repeat(40)
    );
    let text = format!("{EXAMPLE}{deps}{native}");
    let manifest = Manifest::parse(&text).unwrap();
    assert!(
        matches!(&manifest.dependencies["addon"], Dependency::Detailed(details) if details.optional)
    );
    assert!(matches!(
        &manifest.dependencies["create"],
        Dependency::Version(_)
    ));
    for bad in [
        text.replace("^6.0", "not semver"),
        text.replace("create =", "\"not-valid\" ="),
        text.replace("optional = true", "unexpected = true"),
    ] {
        assert!(Manifest::parse(&bad).is_err());
    }
}

#[test]
fn a_code_mod_carries_one_bounded_uses_table_and_a_content_mod_none() {
    let native = format!(
        "{EXAMPLE}[native]\ngame = '26.3'\nsunrise = '{}'\nmixins = '{}'\n",
        "a".repeat(40),
        "b".repeat(40)
    );
    let cargo = b"[package]\nname = 'ruby'\n".as_slice();
    let lib = b"pub fn f() {}\n".as_slice();
    let table = br#"{"schema":1,"made_for":"x","entries":[]}"#.as_slice();
    let bytes = Package::write_files(
        &native,
        &[
            ("Cargo.toml", cargo),
            ("src/lib.rs", lib),
            (USES_FILE, table),
        ],
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    Package::read_zip(&mut zip).unwrap();
    let mut read = Vec::new();
    zip.by_name(USES_FILE)
        .unwrap()
        .read_to_end(&mut read)
        .unwrap();
    assert_eq!(read, table);
    let large = vec![b' '; MAX_USES as usize + 1];
    assert!(
        Package::write_files(
            &native,
            &[
                ("Cargo.toml", cargo),
                ("src/lib.rs", lib),
                (USES_FILE, &large)
            ],
        )
        .unwrap_err()
        .contains(USES_FILE)
    );
    // A content mod has no code to scan, so no table either, written or read.
    assert!(Package::write_files(EXAMPLE, &[(USES_FILE, table)]).is_err());
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in [(MANIFEST, EXAMPLE.as_bytes()), (USES_FILE, table)] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    let mut archive = zip::ZipArchive::new(zip.finish().unwrap()).unwrap();
    let error = Package::read_zip(&mut archive)
        .err()
        .expect("a content package's uses.json is refused");
    assert!(error.contains(USES_FILE), "{error}");
}

#[test]
fn flat_native_archive_round_trips_through_shared_writer() {
    let manifest = format!(
        "{EXAMPLE}[native]\ngame = '26.3'\nsunrise = '{}'\nmixins = '{}'\n",
        "a".repeat(40),
        "b".repeat(40)
    );
    let files: &[(&str, &[u8])] = &[
        (
            "Cargo.toml",
            b"[package]\nname = 'other'\nversion = '2.0.0'\n",
        ),
        ("src/lib.rs", b"pub fn hooks() {}"),
    ];
    let bytes = Package::write_files(&manifest, files).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let package = Package::read_zip(&mut zip).unwrap();
    assert_eq!(package.manifest.id, "ruby");
    assert_eq!(package.manifest.version.to_string(), "0.1.0");
    assert_eq!(package.manifest.native.unwrap().game, "26.3");
    assert!(
        Package::write_files(&manifest, &[("Cargo.toml", b""), ("src/../bad.rs", b"")]).is_err()
    );
    assert!(
        Package::write_files(
            &manifest,
            &[
                ("Cargo.toml", b""),
                ("src/lib.rs", b""),
                ("src/lib.rs", b"")
            ]
        )
        .is_err()
    );
}

#[test]
fn flat_project_packs_only_selected_safe_files() {
    let root = std::env::temp_dir().join(format!("sunrise-flat-pack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
    std::fs::create_dir_all(root.join("target/generated")).unwrap();
    let manifest = format!(
        "{EXAMPLE}[native]\ngame = '26.3'\nsunrise = '{}'\nmixins = '{}'\n",
        "a".repeat(40),
        "b".repeat(40)
    );
    std::fs::write(root.join(MANIFEST), manifest).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'other'\nversion = '2.0.0'\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn hooks() {}\n").unwrap();
    let registry_lock = "version = 4\n[[package]]\nname = 'itoa'\nversion = '1.0.18'\n";
    std::fs::write(root.join("Cargo.lock"), registry_lock).unwrap();
    std::fs::write(root.join(".git/hooks/pre-commit"), "#!/bin/sh\n").unwrap();
    std::fs::write(root.join("target/generated/bad.rs"), "#!/bin/sh\n").unwrap();
    // The SDK the mod tool fetched: the API's own crate, which is never the mod's.
    let sdk = root.join(".sunrise/sdk/api/crates/sunrise-mod-api/src");
    std::fs::create_dir_all(&sdk).unwrap();
    std::fs::write(sdk.join("lib.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join(".sunrise/sdk/sdk.json"), "{}").unwrap();
    let bytes = Package::pack_directory(&root).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(Package::read_zip(&mut zip).unwrap().manifest.id, "ruby");
    assert!(zip.by_name("Cargo.toml").is_ok());
    assert!(zip.by_name("src/lib.rs").is_ok());
    let mut packed_lock = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("Cargo.lock").unwrap(), &mut packed_lock)
        .unwrap();
    assert_eq!(packed_lock, registry_lock);
    assert!(zip.by_name(".git/hooks/pre-commit").is_err());
    assert!(zip.by_name("target/generated/bad.rs").is_err());
    assert!(
        zip.file_names_raw()
            .all(|name| !name.starts_with(b".sunrise")),
        "{:?}",
        zip.file_names_raw()
            .map(String::from_utf8_lossy)
            .collect::<Vec<_>>()
    );
    assert!(!source_file(
        ".sunrise/sdk/api/crates/sunrise-mod-api/src/lib.rs"
    ));
    std::fs::write(root.join("src/lib.rs"), b"MZ executable").unwrap();
    assert!(
        Package::pack_directory(&root)
            .unwrap_err()
            .contains("unsupported executable")
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// A port written one file per class keeps more than 511 source files, far under the byte cap.
#[test]
fn a_native_project_of_many_source_files_packs_and_reads() {
    let root = std::env::temp_dir().join(format!("sunrise-many-sources-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/classes")).unwrap();
    let manifest = format!(
        "{EXAMPLE}[native]\ngame = '26.3'\nsunrise = '{}'\nmixins = '{}'\n",
        "a".repeat(40),
        "b".repeat(40)
    );
    std::fs::write(root.join(MANIFEST), manifest).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = 'ruby'\n").unwrap();
    std::fs::write(root.join("src/lib.rs"), "mod classes;\n").unwrap();
    for i in 0..700 {
        std::fs::write(root.join(format!("src/classes/c{i}.rs")), "pub struct C;\n").unwrap();
    }
    let bytes = Package::pack_directory(&root).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(Package::read_zip(&mut zip).unwrap().manifest.id, "ruby");
    assert!(zip.by_name("src/classes/c699.rs").is_ok());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn package_kind_requires_matching_source_and_native_declaration() {
    let root = std::env::temp_dir().join(format!("sunrise-package-kind-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(MANIFEST), EXAMPLE).unwrap();
    assert!(Package::read(&root).is_ok());
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'unrelated'\nversion = '7.0.0'\n",
    )
    .unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    assert!(Package::read(&root).is_err());
    let native = format!(
        "[native]\ngame = '26.3'\nsunrise = '{}'\nmixins = '{}'\n",
        "a".repeat(40),
        "b".repeat(40)
    );
    std::fs::write(root.join(MANIFEST), format!("{EXAMPLE}{native}")).unwrap();
    assert_eq!(Package::read(&root).unwrap().manifest.id, "ruby");
    std::fs::create_dir(root.join("native")).unwrap();
    assert!(Package::read(&root).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn checks_extra_namespaces() {
    let with = |list: &str| Manifest::parse(&format!("{EXAMPLE}namespaces = [{list}]\n"));
    let m = with("\"kaisyn\", \"towns_and_towers\"").unwrap();
    let owned: Vec<&str> = m.owned_namespaces().collect();
    assert_eq!(owned, ["ruby", "kaisyn", "towns_and_towers"]);
    for reserved in ["minecraft", "c", "sunrise"] {
        let e = with(&format!("\"{reserved}\"")).unwrap_err();
        assert!(e.contains("not a mod's to claim"), "{e}");
    }
    assert!(with("\"ruby\"").unwrap_err().contains("twice"));
    assert!(with("\"a\", \"a\"").unwrap_err().contains("twice"));
    assert!(with("\"Kaisyn\"").unwrap_err().contains("is not"));
    assert!(with("\"\"").unwrap_err().contains("is not"));
}

#[test]
fn reserved_namespaces_cannot_be_the_mod_id() {
    for id in RESERVED_NAMESPACES {
        let manifest = EXAMPLE.replace("id = \"ruby\"", &format!("id = {id:?}"));
        let error = Manifest::parse(&manifest).unwrap_err();
        assert!(
            error.contains(MANIFEST) && error.contains(id) && error.contains("reserved namespace"),
            "{error}"
        );
    }
}

#[test]
fn a_page_of_the_mod_site_cannot_be_the_mod_id() {
    for id in ["mine", "account", "policy", "submit", "index", "edit"] {
        let manifest = EXAMPLE.replace("id = \"ruby\"", &format!("id = {id:?}"));
        let error = Manifest::parse(&manifest).unwrap_err();
        assert!(
            error.contains(MANIFEST) && error.contains(id) && error.contains("mod site"),
            "{error}"
        );
    }
}

#[test]
fn the_mods_screen_fields_are_optional_and_links_must_be_http() {
    let m = Manifest::parse(EXAMPLE).unwrap();
    assert!(m.license.is_empty() && m.credits.is_empty());
    assert!(m.license_url.is_none() && m.homepage.is_none() && m.issues.is_none());
    let full = format!(
        "{EXAMPLE}license = \"MIT\"
credits = \"Alex\"
homepage = \"https://example.org\"
issues = \"http://example.org/issues\"
"
    );
    let m = Manifest::parse(&full).unwrap();
    assert_eq!((m.license.as_str(), m.credits.as_str()), ("MIT", "Alex"));
    assert_eq!(m.homepage.as_deref(), Some("https://example.org"));
    for field in ["license_url", "homepage", "issues"] {
        let e = Manifest::parse(&format!(
            "{EXAMPLE}{field} = \"file:///etc/passwd\"
"
        ))
        .unwrap_err();
        assert!(e.contains(field), "{e}");
    }
}

#[test]
fn content_round_trips_and_executable_files_are_refused() {
    use std::io::Write;
    let root = std::env::temp_dir().join(format!("sunrise-mods-{}", std::process::id()));
    let dir = root.join("folder");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST), EXAMPLE).unwrap();
    let archive = root.join("ruby.smod");
    std::fs::write(&archive, Package::write_files(EXAMPLE, &[]).unwrap()).unwrap();
    assert_eq!(Package::read(&archive).unwrap().manifest.id, "ruby");
    assert_eq!(Package::read(&dir).unwrap().manifest.id, "ruby");
    let cases: &[(&str, &[u8])] = &[
        ("mod.exe", b""),
        ("./mod.exe", b"MZ"),
        ("MOD.EXE", b""),
        ("sub/mod.exe", b""),
        ("assets/ruby/CODE.ExE", b""),
        ("assets/ruby/CODE.WaSm", b""),
        ("assets/ruby/model.bin", b"\0asm"),
        ("assets/ruby/native.bin", b"\x7fELF"),
        ("assets/ruby/windows.bin", b"MZ"),
        ("assets/ruby/mac.bin", b"\xcf\xfa\xed\xfe"),
        ("assets/ruby/script", b"#!/bin/sh"),
        ("assets/ruby/program.EXE", b""),
    ];
    for (name, bytes) in cases {
        let file = dir.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, bytes).unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(MANIFEST, options).unwrap();
        zip.write_all(EXAMPLE.as_bytes()).unwrap();
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
        zip.finish().unwrap();
        let error = Package::read(&archive).err().unwrap();
        assert!(
            error.contains("unsupported executable file"),
            "{name}: {error}"
        );
        if name.starts_with("assets/") {
            let error = Package::read(&dir).err().unwrap();
            assert!(
                error.contains("unsupported executable file"),
                "{name}: {error}"
            );
        } else {
            assert!(Package::read(&dir).is_ok(), "unpackaged {name}");
        }
        std::fs::remove_file(file).unwrap();
    }
    for (name, bytes) in cases.iter().filter(|(name, _)| name.starts_with("assets/")) {
        assert!(
            Package::write_files(EXAMPLE, &[(name, bytes)]).is_err(),
            "{name}"
        );
    }
    // Native bundle sources can start with Rust inner attributes, not a script shebang.
    assert!(
        Package::write_files(
            EXAMPLE,
            &[("assets/ruby/source.txt", b"#![allow(dead_code)]")]
        )
        .is_ok()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "daily"]
fn project_folder_loads_without_packaging_git_hooks() {
    let root = std::env::temp_dir().join(format!("sunrise-mod-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&root)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(root.join(MANIFEST), EXAMPLE).unwrap();
    std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
    std::fs::write(root.join(".git/hooks/pre-commit"), b"#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.join(".git/hooks/pre-commit"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    std::fs::create_dir_all(root.join("assets/ruby")).unwrap();
    std::fs::write(root.join("assets/ruby/icon.png"), b"image").unwrap();
    assert_eq!(Package::read(&root).unwrap().manifest.id, "ruby");
    std::fs::write(root.join("assets/ruby/icon.png"), b"MZ").unwrap();
    assert!(
        Package::read(&root)
            .err()
            .unwrap()
            .contains("unsupported executable")
    );
    std::fs::remove_file(root.join("assets/ruby/icon.png")).unwrap();
    std::fs::create_dir_all(root.join("assets/minecraft/lang")).unwrap();
    std::fs::write(root.join("assets/minecraft/lang/en_us.json"), "{}").unwrap();
    let package = Package::read(&root).unwrap();
    assert!(crate::Content::read(&root, &package.manifest).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn structure_nbt_is_data_but_not_an_executable_exception() {
    let name = "data/ruby/structure/hut.nbt";
    // An uncompressed, named NBT root containing an empty compound.
    let nbt = [10u8, 0, 0, 0];
    assert!(check_file(name, 0o100644, || Ok(std::io::Cursor::new(nbt))).is_ok());
    assert!(matches!(
        check_file(name, 0o100755, || Ok(std::io::Cursor::new(nbt))),
        Err(ContentFileError::Executable(_))
    ));
    for header in [b"\x7fELF", b"MZ!!", b"\xcf\xfa\xed\xfe"] {
        assert!(matches!(
            check_file(name, 0o100644, || Ok(std::io::Cursor::new(header))),
            Err(ContentFileError::Executable(_))
        ));
    }
}

#[test]
fn archive_links_and_executable_modes_are_refused() {
    use std::io::Write;
    let root = std::env::temp_dir().join(format!("sunrise-mods-kinds-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("ruby.smod");
    for link in [false, true] {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        // Made by a Unix tool on every OS: the writer's default on Windows is DOS attributes,
        // which hold no mode to refuse.
        let options = zip::write::SimpleFileOptions::default().system(zip::System::Unix);
        zip.start_file(MANIFEST, options).unwrap();
        zip.write_all(EXAMPLE.as_bytes()).unwrap();
        if link {
            zip.add_symlink("assets/ruby/link", "missing", options)
                .unwrap();
        } else {
            zip.start_file("assets/ruby/program", options.unix_permissions(0o755))
                .unwrap();
            zip.write_all(b"ordinary bytes").unwrap();
        }
        zip.finish().unwrap();
        let error = Package::read(&path).err().unwrap();
        assert!(
            error.contains(if link {
                "unsupported file type"
            } else {
                "unsupported executable"
            }),
            "{error}"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn folder_links_and_executable_modes_are_refused() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root =
        std::env::temp_dir().join(format!("sunrise-mods-folder-kinds-{}", std::process::id()));
    let dir = root.join("folder");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST), EXAMPLE).unwrap();
    std::fs::create_dir_all(dir.join("assets/ruby")).unwrap();
    let file = dir.join("assets/ruby/program");
    std::fs::write(&file, b"ordinary bytes").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = Package::read(&dir).err().unwrap();
    assert!(error.contains("unsupported executable"), "{error}");
    std::fs::remove_file(&file).unwrap();
    for name in ["assets/ruby/mod.exe", "assets/ruby/nested/link"] {
        let link = dir.join(name);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink("missing", &link).unwrap();
        let error = Package::read(&dir).err().unwrap();
        assert!(error.contains("unsupported file type"), "{error}");
        std::fs::remove_file(link).unwrap();
    }
    std::fs::remove_dir_all(dir.join("assets")).unwrap();
    symlink("missing", dir.join("assets")).unwrap();
    assert!(
        Package::read(&dir)
            .err()
            .unwrap()
            .contains("unsupported file type")
    );
    let link = root.join("linked-folder");
    symlink(&dir, &link).unwrap();
    let error = Package::read(&link).err().unwrap();
    assert!(error.contains("unsupported file type"), "{error}");
    std::fs::remove_dir_all(root).unwrap();
}

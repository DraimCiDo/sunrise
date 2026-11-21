use super::*;

#[test]
fn decoded_root_requirements_validate_owners_at_the_read_boundary() {
    let valid = serde_json::json!({
        "owners": ["zeta", "alpha"],
        "jar": {"id":"original", "version":"1", "sha512":"a".repeat(128)}
    });
    let requirement: JarRequirement = serde_json::from_value(valid.clone()).unwrap();
    assert_eq!(requirement.owners, ["zeta", "alpha"]);
    for owners in [
        serde_json::json!([]),
        serde_json::json!(["alpha", "alpha"]),
        serde_json::json!(["UPPER"]),
        serde_json::json!(["../alpha"]),
    ] {
        let mut invalid = valid.clone();
        invalid["owners"] = owners;
        assert!(serde_json::from_value::<JarRequirement>(invalid).is_err());
    }
    let mut invalid = valid.clone();
    invalid["jar"]["sha512"] = serde_json::json!("A".repeat(128));
    assert!(serde_json::from_value::<JarRequirement>(invalid).is_err());
    let mut invalid = valid;
    invalid["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<JarRequirement>(invalid).is_err());
}

#[test]
fn shared_jar_requirements_keep_all_owners_in_load_order() {
    let jar = JarRef::new("original".into(), "1".into(), "a".repeat(128)).unwrap();
    let requirements = collect_jar_requirements([
        ("zeta", std::slice::from_ref(&jar)),
        ("alpha", [jar.clone(), jar.clone()].as_slice()),
        ("beta", std::slice::from_ref(&jar)),
    ])
    .unwrap();
    assert_eq!(requirements.len(), 1);
    assert_eq!(requirements[0].jar, jar);
    assert_eq!(requirements[0].owners, ["zeta", "alpha", "beta"]);
}

#[test]
fn manifest_jar_list_defaults_empty_and_survives_package_roundtrip_in_order() {
    use super::super::{Manifest, Package};
    let base =
        "id = 'port'\nname = 'Port'\nversion = '1'\napi = '0.2'\ncapabilities = ['registries']\n";
    assert!(Manifest::parse(base).unwrap().jar.is_empty());
    let text = format!(
        "{base}[[jar]]\nid = 'first_jar'\nversion = '1'\nsha512 = '{}'\n[[jar]]\nid = 'fabric-api'\nversion = '2'\nsha512 = '{}'\n",
        "0".repeat(128),
        "1".repeat(128)
    );
    let manifest = Manifest::parse(&text).unwrap();
    assert_eq!(
        manifest.jar.iter().map(JarRef::id).collect::<Vec<_>>(),
        ["first_jar", "fabric-api"]
    );
    let bytes = Package::write_files(&text, &[]).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(Package::read_zip(&mut archive).unwrap().manifest, manifest);
    assert!(Manifest::parse(&format!("{text}loader = 'fabric'\n")).is_err());
}

#[test]
fn manifest_refs_preserve_original_ids_and_refuse_invalid_boundary_values() {
    let hash = "0123456789abcdef".repeat(8);
    for id in ["fabric-api", "neo_mod", "_1", "1mod", &"a".repeat(64)] {
        let text = format!("id = {id:?}\nversion = '1.2+build'\nsha512 = {hash:?}\n");
        let jar: JarRef = toml::from_str(&text).unwrap();
        assert_eq!(jar.id, id);
        assert_eq!(
            toml::from_str::<JarRef>(&toml::to_string(&jar).unwrap()).unwrap(),
            jar
        );
        assert!(toml::from_str::<JarRef>(&format!("{text}loader = 'fabric'\n")).is_err());
    }
    for id in [
        "",
        "a",
        "Fabric",
        "-bad",
        "1-mod",
        "bad.id",
        "bad/id",
        &"a".repeat(65),
    ] {
        assert!(
            JarRef::new(id.into(), "1".into(), hash.clone()).is_err(),
            "{id}"
        );
    }
    assert!(JarRef::new("good".into(), String::new(), hash.clone()).is_err());
    for bad_hash in [
        "a".repeat(127),
        "a".repeat(129),
        "A".repeat(128),
        "g".repeat(128),
    ] {
        let text = format!("id = 'good'\nversion = '1'\nsha512 = {bad_hash:?}\n");
        assert!(toml::from_str::<JarRef>(&text).is_err());
    }
}

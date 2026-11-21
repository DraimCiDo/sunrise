use crate::{Package, content::Content};

#[test]
fn only_living_types_accept_an_explicit_peaceful_setting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("peaceful.smod");
    let name = "data/bird/sunrise/entity_type/mob.json";
    let manifest = r#"id = "bird"
name = "Bird"
version = "1.0"
api = "0.2"
side = "both"
capabilities = ["registries"]
"#;
    let read = |declaration: &str| {
        std::fs::write(
            &path,
            Package::write_files(manifest, &[(name, declaration.as_bytes())]).unwrap(),
        )
        .unwrap();
        let package = Package::read(&path).unwrap();
        Content::read(&path, &package.manifest)
    };
    let living =
        read(r#"{"template":"minecraft:wandering_trader","allowed_in_peaceful":false}"#).unwrap();
    assert_eq!(living.entity_types[0].allowed_in_peaceful, Some(false));
    for value in ["false", "true", "null"] {
        let error = read(&format!(
                r#"{{"template":"minecraft:snowball","item":"bird:ball","allowed_in_peaceful":{value}}}"#
            ))
            .unwrap_err();
        assert!(error.contains(name), "{error}");
        if value != "null" {
            assert!(error.contains("allowed_in_peaceful"), "{error}");
        }
    }
    for value in ["null", "\"false\""] {
        let error = read(&format!(
            r#"{{"template":"minecraft:wandering_trader","allowed_in_peaceful":{value}}}"#
        ))
        .unwrap_err();
        assert!(error.contains(name), "{error}");
    }
}

#[test]
fn shadow_admission_rejects_negative_and_nonfinite_values_with_filename() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shadow.smod");
    let name = "data/bird/sunrise/entity_type/mob.json";
    let manifest = r#"id = "bird"
name = "Bird"
version = "1.0"
api = "0.2"
side = "both"
capabilities = ["registries"]
"#;
    for key in ["shadow_radius", "shadow_strength"] {
        for value in ["-1", "1e100", "1e999", "NaN"] {
            let file = format!(r#"{{"template":"minecraft:zombie","{key}":{value}}}"#);
            std::fs::write(
                &path,
                Package::write_files(manifest, &[(name, file.as_bytes())]).unwrap(),
            )
            .unwrap();
            let package = Package::read(&path).unwrap();
            let error = Content::read(&path, &package.manifest).unwrap_err();
            assert!(error.contains(name), "{error}");
        }
    }
}

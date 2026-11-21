use super::*;

#[test]
fn a_bad_value_condition_refuses_the_typed_layer_before_any_mutation() {
    let files = vec![("data_maps/neoforge:item/fuel".to_owned(), vec![
            br#"{"values":{"minecraft:stick":2,"minecraft:coal":3}}"#.to_vec(),
            br#"{"replace":true,"values":{"minecraft:blaze_rod":4,"minecraft:planks":{"value":5,"neoforge:conditions":[{"type":"neoforge:or","values":[{"type":"neoforge:always"},{"type":"absent:fuel"}]}]}},"remove":["minecraft:coal"]}"#.to_vec(),
            br#"{"values":{"minecraft:lava_bucket":6}}"#.to_vec(),
        ])];
    let conditions = sunrise_world::conditions::LoadConditions::new([]);
    let overlay = sunrise_world::loot::PackOverlay {
        files: &files,
        conditions: &conditions,
        ..sunrise_world::loot::PackOverlay::default()
    };
    let expected = BTreeMap::from([
        ("minecraft:stick".to_owned(), 2),
        ("minecraft:coal".to_owned(), 3),
        ("minecraft:lava_bucket".to_owned(), 6),
    ]);
    let (values, _) = load(
        "neoforge:fuel",
        "item",
        BTreeMap::new(),
        &overlay,
        &Tags::new(),
        |raw| Ok(entry_value(raw)?.as_i64()),
        |_| true,
    );
    assert_eq!(values, expected);
    let (values, _) = load_numeric(
        "neoforge:fuel",
        "item",
        BTreeMap::new(),
        &overlay,
        &Tags::new(),
        NumberCodec {
            field: "burn_time",
            decode: |raw: &RawValue| {
                serde_json::from_str::<i64>(raw.get()).map_err(|error| error.to_string())
            },
        },
        |_| true,
    );
    assert_eq!(values, expected);
    assert_eq!(conditions.check_errors(), Ok(()));
}

#[test]
fn typed_maps_ignore_file_conditions_and_keep_value_conditions() {
    let files = vec![(
            "data_maps/test:item/fuel".to_owned(),
            vec![br#"{"neoforge:conditions":[{"type":"neoforge:never"}],"values":{"minecraft:stick":2,"minecraft:coal":{"neoforge:conditions":[{"type":"neoforge:never"}],"value":3}}}"#.to_vec()],
        )];
    let overlay = sunrise_world::loot::PackOverlay {
        files: &files,
        ..sunrise_world::loot::PackOverlay::default()
    };
    let (values, applied) = load(
        "test:fuel",
        "item",
        BTreeMap::new(),
        &overlay,
        &Tags::new(),
        |raw| Ok(Some(entry_value(raw)?.as_i64().unwrap())),
        |_| true,
    );
    assert!(applied);
    assert_eq!(values, BTreeMap::from([("minecraft:stick".to_owned(), 2)]));
    let (values, applied) = load_numeric(
        "test:fuel",
        "item",
        BTreeMap::new(),
        &overlay,
        &Tags::new(),
        NumberCodec {
            field: "burn_time",
            decode: |raw: &RawValue| {
                serde_json::from_str::<i64>(raw.get()).map_err(|error| error.to_string())
            },
        },
        |_| true,
    );
    assert!(applied);
    assert_eq!(values, BTreeMap::from([("minecraft:stick".to_owned(), 2)]));
}

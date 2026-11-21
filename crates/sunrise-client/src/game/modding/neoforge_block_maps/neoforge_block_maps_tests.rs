use super::*;
use serde_json::json;

#[test]
fn pack_files_give_the_block_maps() {
    let file = |value: serde_json::Value| vec![serde_json::to_vec(&value).unwrap()];
    let files = vec![
        (
            "data_maps/neoforge:block/flammable".to_owned(),
            file(json!({"values": {"minecraft:stone": {"ignite_odds": 5, "burn_odds": 20}}})),
        ),
        (
            "data_maps/neoforge:block/waxables".to_owned(),
            file(json!({"values": {"test:tin": {"waxed": "test:waxed_tin"}}})),
        ),
    ];
    let overlay = sunrise_world::loot::PackOverlay {
        files: &files,
        ..sunrise_world::loot::PackOverlay::default()
    };
    let ids = ["minecraft:stone", "test:tin", "test:waxed_tin"].map(String::from);
    let items = sunrise_client::items::Items::empty();
    let registries = WorldRegistries::new(
        [("minecraft:block", ids.as_slice())],
        None,
        &items,
        std::path::Path::new(""),
        &overlay,
    );
    let block_maps = load(&overlay, &registries, |_| 0).unwrap();
    assert_eq!(block_maps.flammable("minecraft:stone"), Some((5, 20)));
    assert_eq!(block_maps.waxed("test:tin"), Some("test:waxed_tin"));
    assert_eq!(block_maps.next_oxidation("test:tin"), None);
    let none = load(&sunrise_world::loot::PackOverlay::NONE, &registries, |_| 0);
    assert!(none.is_none());
}

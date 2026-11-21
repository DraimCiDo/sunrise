use super::*;
#[test]
fn context_shape_selectors_and_tables_are_validated() {
    for selector in ["item", "player", "living", "other"] {
        for channel in ["outline_context_shapes", "collision_context_shapes"] {
            let text = format!(
                r#"{{"template":"minecraft:stone","{channel}":{{"{selector}":{{"default":[]}}}}}}"#
            );
            let _: BlockFile = serde_json::from_str(&text).unwrap();
        }
    }
    for selector in ["none", "Item", "minecraft:item", "entity"] {
        let text = format!(
            r#"{{"template":"minecraft:stone","collision_context_shapes":{{"{selector}":{{"default":[]}}}}}}"#
        );
        assert!(serde_json::from_str::<BlockFile>(&text).is_err());
    }
    let mut block = Block {
        template: "minecraft:stone".into(),
        ..Default::default()
    };
    let bad = BlockShapes(StateValues {
        property: String::new(),
        properties: vec![],
        values: BTreeMap::new(),
        default: Some(vec![[1., 0., 0., 0., 1., 1.]]),
    });
    block
        .outline_context_shapes
        .insert(ShapeEntityKind::Player, bad.clone());
    assert!(check_block(&block).is_err());
    block.outline_context_shapes.clear();
    block
        .collision_context_shapes
        .insert(ShapeEntityKind::Item, bad.clone());
    assert!(check_block(&block).is_err());
    block.collision_context_shapes.clear();
    block.interaction_shapes = Some(bad);
    assert!(check_block(&block).is_err());
}

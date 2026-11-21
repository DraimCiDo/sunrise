use super::*;

/// Two tags that remove each other resolve without an overflow, and the
/// edge toward the later id is ignored, independently of the starting tag.
#[test]
fn mutual_removals_resolve_without_an_overflow() {
    let tags: BTreeMap<String, String> = BTreeMap::from([
        (
            "test:a".into(),
            r##"{"values": ["test:one"], "remove": ["#test:b"]}"##.into(),
        ),
        (
            "test:b".into(),
            r##"{"values": ["test:one", "test:two"], "remove": ["#test:a"]}"##.into(),
        ),
    ]);
    let ids: BTreeSet<String> = ["test:one", "test:two"]
        .into_iter()
        .map(String::from)
        .collect();
    let registry = Registry {
        what: "placed feature",
        ids: &ids,
        tags: &tags,
    };
    crate::test_log::install();
    for (start, expected) in [("test:a", "test:one"), ("test:b", "test:two")] {
        crate::test_log::clear();
        let out = registry
            .resolve(&serde_json::json!(format!("#{start}")))
            .unwrap();
        assert_eq!(out, [expected]);
        {
            let lines = crate::test_log::records();
            assert!(
                lines.iter().any(|(_, line)| line.contains("test:a")
                    && line.contains("test:b")
                    && line.contains("only removal edge test:a -> test:b is ignored")),
                "{lines:?}"
            );
        }
    }
}

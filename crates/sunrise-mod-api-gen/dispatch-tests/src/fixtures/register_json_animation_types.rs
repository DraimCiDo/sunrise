mod stubs {
    #[derive(Debug, PartialEq)]
    pub(super) struct JsonAnimationType(pub(super) &'static str);
}

/// The generated dispatch of `register_json_animation_types` names the mod on each
/// registered entry and preserves dispatch order.
#[test]
fn each_registered_animation_type_names_its_registering_mod() {
    struct Registers(&'static [&'static str]);
    #[mod_api::__subscriptions]
    impl Hooks for Registers {
        fn register_json_animation_types(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterJsonAnimationTypesEvent<'_>,
        ) {
            for &id in self.0 {
                event.register(JsonAnimationType(id));
            }
        }
    }
    install(
        vec![
            ModEntry::new("glow", Box::new(Registers(&["glow:pulse", "glow:step"]))),
            ModEntry::new("quiet", Box::new(Registers(&[]))),
            ModEntry::new("bounce", Box::new(Registers(&["bounce:spring"]))),
        ]
        .into(),
    );
    let mut types = Vec::new();
    dispatch_register_json_animation_types(&RegistrationContext("bootstrap"), &mut types);
    let entry = |id, mod_id| Answered {
        answer: JsonAnimationType(id),
        mod_id,
    };
    assert_eq!(
        types,
        [
            entry("glow:pulse", "glow"),
            entry("glow:step", "glow"),
            entry("bounce:spring", "bounce")
        ]
    );
}

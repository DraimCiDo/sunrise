//! Test support with API-owned registration and vocabulary-owned fakes.
pub use mod_vocab::testing::*;

/// Run with these mods and their controller and pose registrations on this thread.
/// The registration probe starts without frozen test Parts. The body can see explicitly
/// installed Parts from its caller; any replacement is restored on scope exit or unwind.
pub fn with_mods<R>(mods: crate::Mods, f: impl FnOnce() -> R) -> R {
    crate::__private::initialize().expect("test registration requires the mod API runtime");
    mod_vocab::testing::with_parts_registration_probe(
        mods,
        crate::parts::registered_controllers,
        |mods, (controllers, poses)| {
            mod_vocab::testing::with_controller_tables(controllers, poses, || {
                mod_vocab::testing::with_dispatch(mods, f)
            })
        },
    )
}

/// Install explicit raw serializer/type pairs for resource-loader fixtures.
/// Each listed ID is declared in both independent registries by its namespace owner.
pub fn with_recipe_types<R>(ids: &[&'static str], f: impl FnOnce() -> R) -> R {
    struct RawTypes(Vec<&'static str>);
    #[mod_api::__subscriptions]
    impl crate::Hooks for RawTypes {
        fn register(&self, r: &mut dyn crate::Registrar, _: &crate::RegistrationContext) {
            for id in &self.0 {
                r.register_recipe_type(id);
                r.register_raw_recipe_serializer(id, id);
            }
        }
    }
    let entries = || {
        let mut groups = std::collections::BTreeMap::<_, Vec<_>>::new();
        for id in ids {
            groups
                .entry(id.split_once(':').unwrap().0)
                .or_default()
                .push(*id);
        }
        groups
            .into_iter()
            .map(|(ns, ids)| crate::ModEntry::new(ns, Box::new(RawTypes(ids))))
            .collect::<Vec<_>>()
    };
    with_mods(entries().into(), || {
        crate::__private::part_tests::install_parts_fixture(&entries()).unwrap();
        f()
    })
}

#[cfg(test)]
#[test]
fn a_mod_scope_does_not_freeze_the_next_scopes_registration() {
    struct Stat;
    #[mod_api::__subscriptions]
    impl crate::Hooks for Stat {
        fn register(&self, r: &mut dyn crate::Registrar, _: &crate::RegistrationContext) {
            r.register_custom_stat("steps").unwrap();
        }
    }

    for id in ["first", "second"] {
        let mods: crate::Mods = vec![crate::ModEntry::new(id, Box::new(Stat))].into();
        with_mods(mods.clone(), || {
            crate::__private::part_tests::install_parts_fixture(&mods).unwrap();
            assert_eq!(
                mod_vocab::__private::custom_stat_declarations(),
                [format!("{id}:steps")]
            );
        });
    }
}

/// Run a fixture with one real attachment registration. Fetch its key after setting
/// up a game, since game setup can run registration again and issue a new key.
pub fn with_attachment_fixture<T: Send + Sync + 'static, R>(
    owner: &'static str,
    descriptor: impl Fn() -> crate::AttachmentDescriptor<T> + Send + Sync + 'static,
    run: impl FnOnce(&dyn Fn() -> crate::AttachmentKey<T>) -> R,
) -> R {
    use std::sync::{Arc, Mutex};
    struct Owner<D, T: 'static> {
        descriptor: D,
        issued: Arc<Mutex<Option<crate::AttachmentKey<T>>>>,
    }
    impl<T, D> crate::Hooks for Owner<D, T>
    where
        T: Send + Sync + 'static,
        D: Fn() -> crate::AttachmentDescriptor<T> + Send + Sync + 'static,
    {
        fn subscribed(&self) -> &'static [Option<crate::HookId>] {
            &[]
        }
        fn register(&self, registrar: &mut dyn crate::Registrar, _: &crate::RegistrationContext) {
            *self.issued.lock().unwrap() = Some(
                registrar
                    .register_attachment_type((self.descriptor)())
                    .unwrap(),
            );
        }
    }
    let issued = Arc::new(Mutex::new(None));
    let mods: crate::Mods = vec![crate::ModEntry::new(
        owner,
        Box::new(Owner {
            descriptor,
            issued: issued.clone(),
        }),
    )]
    .into();
    with_mods(mods.clone(), || {
        crate::__private::part_tests::install_parts_fixture(&mods).unwrap();
        run(&|| issued.lock().unwrap().clone().unwrap())
    })
}

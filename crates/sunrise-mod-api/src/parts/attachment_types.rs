//! Load-time attachment declarations, before any registration is installed.
use super::*;

impl AttachmentTypesRegistrar for PartRegistrar {
    fn submit_attachment(
        &mut self,
        descriptor: ErasedAttachmentDescriptor,
    ) -> Result<RegisteredAttachment, String> {
        let id = descriptor.id();
        let result = (|| {
            // Only the active registration scope can collect a new epoch while
            // the previous immutable snapshot remains installed.
            if mod_vocab::__private::parts_installation::parts_installed()
                && !REGISTERING_KEYS.with_borrow(Option::is_some)
            {
                return Err("declarations are frozen".to_owned());
            }
            let invalid = "explicit lowercase namespaced id required";
            let (namespace, name) = id
                .split_once(':')
                .filter(|(namespace, name)| !namespace.is_empty() && !name.is_empty())
                .ok_or(invalid)?;
            crate::Identifier::from_parts(namespace, name).map_err(|_| invalid)?;
            if self
                .attachment_types
                .get(namespace)
                .is_some_and(|types| types.contains_key(name))
            {
                return Err(format!("already declared by package {namespace}"));
            }
            if namespace != self.mod_id {
                return Err(format!(
                    "namespace belongs to package {namespace}, not {}",
                    self.mod_id
                ));
            }
            let registered =
                check_attachment_descriptor(descriptor, namespace, name).map_err(|error| {
                    match error {
                        crate::AttachmentError::InvalidCopyPolicy => {
                            "copy-on-death requires a codec"
                        }
                        crate::AttachmentError::InvalidSyncPolicy => "sync requires a codec",
                        _ => "invalid attachment descriptor",
                    }
                })?;
            self.attachment_types
                .entry(namespace)
                .or_default()
                .insert(name, registered.clone());
            Ok(registered)
        })();
        result.map_err(|reason| {
            let message = format!("package {} attachment {id}: {reason}", self.mod_id);
            self.refused.push(message.clone());
            message
        })
    }

    fn lookup_attachment(&self, id: &str) -> Result<RegisteredAttachment, crate::AttachmentError> {
        id.split_once(':')
            .and_then(|(namespace, name)| self.attachment_types.get(namespace)?.get(name))
            .cloned()
            .ok_or_else(|| crate::AttachmentError::Undeclared {
                package_id: self.mod_id.into(),
                attachment_id: id.into(),
            })
    }
}

impl PartRegistrar {
    // Only synced declarations affect join admission; a dependent may still look one up.
    pub(crate) fn validate_attachment_admission(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        for package in crate::load_state::current_packages()
            .iter()
            .filter(|p| p.side() == crate::Side::Server)
        {
            if let Some(types) = self.attachment_types.get(package.id()) {
                for name in types
                    .iter()
                    .filter(|(_, record)| record.is_synced())
                    .map(|(name, _)| name)
                    .collect::<std::collections::BTreeSet<_>>()
                {
                    errors.push(format!("package {} attachment {}:{name}: a side = \"server\" package cannot declare a synced attachment", package.id(), package.id()));
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const MANA_CODEC: crate::AttachmentCodec<u32> = crate::AttachmentCodec {
        encode: |n| Ok([("n".into(), crate::Tag::Long(i64::from(*n)))].into()),
        decode: |saved| match saved.get(&"n".into()) {
            Some(crate::Tag::Long(n)) => {
                u32::try_from(*n).map_err(|_| crate::AttachmentError::Decode)
            }
            _ => Err(crate::AttachmentError::Decode),
        },
    };
    #[derive(Default)]
    struct Store(crate::Attachments);
    impl crate::Host for Store {}
    mod_api::impl_host_areas!(Store, except(mod_state));
    impl crate::ModStateHost for Store {
        fn attachment_query(
            &self,
            _: &str,
            _: &str,
        ) -> Result<crate::AttachmentBatch, crate::AttachmentError> {
            Ok(sunrise_mod_runtime::attachments::AttachmentBatch::empty().into())
        }
        fn attachments(&mut self) -> &mut dyn crate::AttachmentStore {
            self
        }
    }
    impl crate::AttachmentHost for Store {
        fn validate_attachment(
            &self,
            namespace: &str,
            name: &str,
            sync: bool,
        ) -> Result<(), crate::AttachmentError> {
            mod_vocab::__private::validate_attachment_use(namespace, name, sync)
        }
        fn attachments(
            &mut self,
            _: crate::Holder,
            _: crate::AttachmentAccess,
        ) -> Result<&mut crate::Attachments, crate::AttachmentError> {
            Ok(&mut self.0)
        }
    }
    #[test]
    fn foreign_attachment_declaration_is_named_and_not_added() {
        let mut registrar = PartRegistrar::new("b");
        assert_eq!(
            (&mut registrar as &mut dyn crate::Registrar)
                .register_attachment_type(crate::AttachmentDescriptor::new("a:mana", || 37u32))
                .err(),
            Some("package b attachment a:mana: namespace belongs to package a, not b".into())
        );
        assert!(registrar.attachment_types.is_empty());
    }
    #[test]
    fn server_synced_attachment_refuses_all_registration_and_plain_is_admitted() {
        crate::load_state::with_test_package_inventory(
            vec![crate::PackageInfo::new(
                "test_pkg".into(),
                "1".into(),
                crate::PackageKind::Code,
                crate::Side::Server,
                vec!["test_pkg".into()],
                vec![],
            )],
            || {
                let mut bad = PartRegistrar::new("test_pkg");
                bad.register_custom_stat("marker").unwrap();
                (&mut bad as &mut dyn crate::Registrar)
                    .register_attachment_type(crate::AttachmentDescriptor::new(
                        "test_pkg:session",
                        || 37u32,
                    ))
                    .unwrap();
                (&mut bad as &mut dyn crate::Registrar)
                    .register_attachment_type(
                        crate::AttachmentDescriptor::new("test_pkg:mana", || 37u32)
                            .serialize(MANA_CODEC)
                            .sync(),
                    )
                    .unwrap();
                assert_eq!(super::super::install_registrar(bad, &crate::parts::tests::rule_schema()), Err(vec!["package test_pkg attachment test_pkg:mana: a side = \"server\" package cannot declare a synced attachment".into()]));
                assert!(mod_vocab::__private::custom_stat_declarations().is_empty());
                assert!(!mod_vocab::__private::parts_installation::parts_installed());
                let _caller =
                    crate::content::crash::enter_native_mod(crate::content::crash::ModCall {
                        id: "test_pkg",
                        hook: "test",
                        part: None,
                    });
                assert!(
                    mod_vocab::__private::validate_attachment_use("test_pkg", "session", false)
                        .is_err()
                );
                let mut good = PartRegistrar::new("test_pkg");
                (&mut good as &mut dyn crate::Registrar)
                    .register_attachment_type(crate::AttachmentDescriptor::new(
                        "test_pkg:mana",
                        || 37u32,
                    ))
                    .unwrap();
                super::super::install_registrar(good, &crate::parts::tests::rule_schema()).unwrap();
                assert_eq!(
                    mod_vocab::__private::validate_attachment_use("test_pkg", "mana", false),
                    Ok(())
                );
            },
        );
    }

    #[test]
    fn package_b_reads_a_declared_mana_and_undeclared_use_names_b() {
        struct Owner;
        impl crate::Hooks for Owner {
            fn subscribed(&self) -> &'static [Option<crate::HookId>] {
                &[]
            }
            fn register(
                &self,
                registrar: &mut dyn crate::Registrar,
                _: &crate::RegistrationContext,
            ) {
                registrar
                    .register_attachment_type(crate::AttachmentDescriptor::new("a:mana", || 37u32))
                    .unwrap();
            }
        }
        let mana = Arc::new(OnceLock::new());
        let packages = [
            crate::ModEntry::new("a", Box::new(Owner)),
            crate::ModEntry::new("b", Box::new(ServerUser(mana.clone()))),
        ];
        crate::__private::install_parts(&packages, &crate::parts::tests::rule_schema(), |_| false)
            .unwrap();
        let _caller = crate::content::crash::enter_native_mod(crate::content::crash::ModCall {
            id: "b",
            hook: "test",
            part: None,
        });
        let mut store = Store::default();
        assert_eq!(
            mana.get().unwrap().get(&mut store, crate::Holder::Level),
            Ok(37)
        );
        assert_eq!(
            mod_vocab::__private::validate_attachment_use("test_pkg", "mana", false),
            Err(crate::AttachmentError::Undeclared {
                package_id: "b".into(),
                attachment_id: "test_pkg:mana".into(),
            })
        );
    }
    #[test]
    fn a_key_from_the_previous_install_cannot_run_its_default_or_mutate() {
        struct Owner {
            issued: Arc<OnceLock<crate::AttachmentKey<u32>>>,
            default: fn() -> u32,
        }
        impl crate::Hooks for Owner {
            fn subscribed(&self) -> &'static [Option<crate::HookId>] {
                &[]
            }
            fn register(
                &self,
                registrar: &mut dyn crate::Registrar,
                _: &crate::RegistrationContext,
            ) {
                let key = registrar
                    .register_attachment_type(crate::AttachmentDescriptor::new(
                        "a:mana",
                        self.default,
                    ))
                    .unwrap();
                assert!(self.issued.set(key).is_ok());
            }
        }
        let install = |default: fn() -> u32| {
            let issued = Arc::new(OnceLock::new());
            let mods = [crate::ModEntry::new(
                "a",
                Box::new(Owner {
                    issued: issued.clone(),
                    default,
                }),
            )];
            crate::__private::install_parts(&mods, &crate::parts::tests::rule_schema(), |_| false)
                .unwrap();
            issued.get().unwrap().clone()
        };
        let mut store = Store::default();
        let holder = crate::Holder::Level;
        let old = install(|| panic!("stale attachment evaluated its default"));
        assert_eq!(old.has(&mut store, holder), Ok(false));
        let current = install(|| 91);
        assert_eq!(old.id(), current.id());
        assert_eq!(
            old.get(&mut store, holder),
            Err(crate::AttachmentError::StaleKey)
        );
        assert_eq!(
            old.set(&mut store, holder, 44),
            Err(crate::AttachmentError::StaleKey)
        );
        assert_eq!(current.get_existing(&mut store, holder), Ok(None));
        assert_eq!(current.get(&mut store, holder), Ok(91));
        assert_eq!(current.set(&mut store, holder, 18), Ok(()));
        assert_eq!(current.get(&mut store, holder), Ok(18));
    }

    #[test]
    fn worldless_attachment_consumers_refuse_a_stale_issued_key() {
        let issue = || {
            let mut registrar = PartRegistrar::new("a");
            let key = (&mut registrar as &mut dyn crate::Registrar)
                .register_attachment_type(
                    crate::AttachmentDescriptor::new("a:mana", || 37_u32)
                        .serialize(MANA_CODEC)
                        .sync(),
                )
                .unwrap();
            super::super::install_registrar(registrar, &crate::parts::tests::rule_schema())
                .unwrap();
            key
        };
        let old = issue();
        let mut store = Store::default();
        old.set(&mut store, crate::Holder::Level, 73).unwrap();
        let batch: crate::AttachmentBatch =
            sunrise_mod_runtime::attachments::AttachmentBatch::empty().into();
        let render = crate::EntityRenderAttachments::from_synced(&store.0);
        let saved = store.0.to_nbt();
        let current = mod_vocab::testing::rebuild_parts(|| Ok::<_, ()>(issue())).unwrap();
        assert_eq!(render.get(&current), Ok(Some(&73)));
        assert_eq!(render.get(&old), Err(crate::AttachmentError::StaleKey));
        assert_eq!(
            batch.query(&old).err(),
            Some(crate::AttachmentError::StaleKey)
        );
        let mut data = crate::ChunkDiskData::from_nbt(saved.clone()).unwrap();
        let refusal = data.set(&old, 99).unwrap_err();
        assert_eq!(refusal.key, "a:mana");
        assert_eq!(refusal.reason, "StaleKey");
        assert_eq!(data.finish(), Err(refusal));
        let mut data = crate::ChunkDiskData::from_nbt(saved).unwrap();
        assert_eq!(data.get(&current), Ok(73));
    }

    #[test]
    fn a_descriptor_that_needs_a_codec_refuses_install_even_if_ignored() {
        let mut registrar = PartRegistrar::new("a");
        let r: &mut dyn crate::Registrar = &mut registrar;
        let _ =
            r.register_attachment_type(crate::AttachmentDescriptor::new("a:sync", || 0u32).sync());
        let _ = r.register_attachment_type(
            crate::AttachmentDescriptor::new("a:copy", || 0u32).copy_on_death(),
        );
        assert!(registrar.attachment_types.is_empty());
        assert_eq!(
            super::super::install_registrar(registrar, &crate::parts::tests::rule_schema()),
            Err(vec![
                "package a attachment a:sync: sync requires a codec".into(),
                "package a attachment a:copy: copy-on-death requires a codec".into(),
            ])
        );
    }

    #[test]
    fn synced_attachment_is_named_to_server_package_admission() {
        let mut registrar = PartRegistrar::new("test_pkg");
        (&mut registrar as &mut dyn crate::Registrar)
            .register_attachment_type(
                crate::AttachmentDescriptor::new("test_pkg:mana", || 37u32)
                    .serialize(MANA_CODEC)
                    .sync(),
            )
            .unwrap();
        super::super::install_registrar(registrar, &crate::parts::tests::rule_schema()).unwrap();
        assert_eq!(
            crate::__private::client_requirement("test_pkg"),
            Some("declares the synced attachment test_pkg:mana".into())
        );
    }
    fn package(id: &str, side: crate::Side) -> crate::PackageInfo {
        crate::PackageInfo::new(
            id.into(),
            "1".into(),
            crate::PackageKind::Code,
            side,
            vec![id.into()],
            vec![],
        )
    }
    struct SyncedOwner;
    impl crate::Hooks for SyncedOwner {
        fn subscribed(&self) -> &'static [Option<crate::HookId>] {
            &[]
        }
        fn register(&self, registrar: &mut dyn crate::Registrar, _: &crate::RegistrationContext) {
            registrar
                .register_attachment_type(
                    crate::AttachmentDescriptor::new("a:mana", || 37u32)
                        .serialize(MANA_CODEC)
                        .sync(),
                )
                .unwrap();
        }
    }
    struct ServerUser(Arc<OnceLock<crate::AttachmentKey<u32>>>);
    impl crate::Hooks for ServerUser {
        fn subscribed(&self) -> &'static [Option<crate::HookId>] {
            &[]
        }
        fn register(&self, registrar: &mut dyn crate::Registrar, _: &crate::RegistrationContext) {
            assert_eq!(
                registrar.attachment_key::<i32>("a:mana").err(),
                Some(crate::AttachmentError::TypeMismatch)
            );
            assert_eq!(
                registrar.attachment_key::<u32>("a:missing").err(),
                Some(crate::AttachmentError::Undeclared {
                    package_id: "b".into(),
                    attachment_id: "a:missing".into(),
                })
            );
            assert!(
                self.0
                    .set(registrar.attachment_key::<u32>("a:mana").unwrap())
                    .is_ok()
            );
        }
    }
    #[test]
    fn server_b_loads_and_uses_a_synced_mana() {
        crate::load_state::with_test_package_inventory(
            vec![
                package("a", crate::Side::Both),
                package("b", crate::Side::Server),
            ],
            || {
                let mana = Arc::new(OnceLock::new());
                let mods = [
                    crate::ModEntry::new("a", Box::new(SyncedOwner)),
                    crate::ModEntry::new("b", Box::new(ServerUser(mana.clone()))),
                ];
                crate::__private::install_parts(&mods, &crate::parts::tests::rule_schema(), |_| {
                    false
                })
                .unwrap();
                assert_eq!(crate::__private::client_requirement("b"), None);
                let _caller =
                    crate::content::crash::enter_native_mod(crate::content::crash::ModCall {
                        id: "b",
                        hook: "test",
                        part: None,
                    });
                let mut store = Store::default();
                let key = mana.get().unwrap();
                assert_eq!(key.get(&mut store, crate::Holder::Level), Ok(37));
                assert_eq!(
                    store.0.take_sync_changes(),
                    vec![sunrise_mod_runtime::attachments::AttachmentSync {
                        namespace: "a".into(),
                        name: "mana".into(),
                        value: Some([("n".into(), crate::Tag::Long(37))].into()),
                    }]
                );
            },
        );
    }
    #[test]
    fn server_declarer_a_is_refused_instead_of_server_user_b() {
        crate::load_state::with_test_package_inventory(
            vec![
                package("a", crate::Side::Server),
                package("b", crate::Side::Server),
            ],
            || {
                let mods = [
                    crate::ModEntry::new("a", Box::new(SyncedOwner)),
                    crate::ModEntry::new("b", Box::new(ServerUser(Arc::default()))),
                ];
                assert_eq!(crate::__private::install_parts(&mods, &crate::parts::tests::rule_schema(), |_| false), Err(vec!["package a attachment a:mana: a side = \"server\" package cannot declare a synced attachment".into()]));
                assert!(!mod_vocab::__private::parts_installation::parts_installed());
            },
        );
    }
    #[test]
    fn load_reports_descriptor_conflict_before_server_admission() {
        crate::load_state::with_test_package_inventory(
            vec![package("test_pkg", crate::Side::Server)],
            || {
                let mut conflict = PartRegistrar::new("test_pkg");
                let r: &mut dyn crate::Registrar = &mut conflict;
                r.register_attachment_type(
                    crate::AttachmentDescriptor::new("test_pkg:mana", || 37u32)
                        .serialize(MANA_CODEC)
                        .sync(),
                )
                .unwrap();
                let refusal = r
                    .register_attachment_type(crate::AttachmentDescriptor::new("a:mana", || 37u32))
                    .err()
                    .unwrap();
                assert_eq!(
                    refusal,
                    "package test_pkg attachment a:mana: namespace belongs to package a, not test_pkg"
                );
                assert_eq!(
                    super::super::install_registrar(conflict, &crate::parts::tests::rule_schema()),
                    Err(vec![refusal])
                );
                assert!(!mod_vocab::__private::parts_installation::parts_installed());
            },
        );
    }
    #[test]
    fn duplicate_declaration_names_the_caller_and_original_owner() {
        let mut registrar = PartRegistrar::new("a");
        (&mut registrar as &mut dyn crate::Registrar)
            .register_attachment_type(
                crate::AttachmentDescriptor::new("a:mana", || 37u32)
                    .serialize(MANA_CODEC)
                    .sync(),
            )
            .unwrap();
        registrar.mod_id = "b";
        assert_eq!(
            (&mut registrar as &mut dyn crate::Registrar)
                .register_attachment_type(crate::AttachmentDescriptor::new("a:mana", || 37u32))
                .err(),
            Some("package b attachment a:mana: already declared by package a".into())
        );
    }
}

//! Client loading-screen registrations and their presentation data.

use crate::{
    ClientHost, DimensionKey, DrawContext, HookId, Identifier,
    RegisterDimensionTransitionScreenEvent,
};

/// Resolved travel keys shared by registration and the client lookup table.
// no wire: engine-side keys for process-local factories.
#[derive(PartialEq, Eq, Hash)]
pub enum TransitionKey {
    Incoming(DimensionKey),
    Outgoing(DimensionKey),
    Conditional {
        to: DimensionKey,
        from: DimensionKey,
    },
}

/// A factory accepted by the scoped registrar, ready to publish without revalidation.
// no wire: owns a process-local factory.
pub struct RegisteredTransitionScreen {
    pub key: TransitionKey,
    pub owner: &'static str,
    pub screen: DimensionTransitionScreen,
}

const REGISTRATION: (HookId, &str) = (
    HookId::RegisterDimensionTransitionScreen,
    "register_dimension_transition_screen",
);

impl RegisteredTransitionScreen {
    /// Engine side of the registration operation; only the active listener can append to its batch.
    pub fn register(
        registrations: &mut Vec<Self>,
        owner: &'static str,
        screen: DimensionTransitionScreen,
    ) -> bool {
        let resolve = |id: &str| {
            Identifier::parse_namespaced(id).map(|_| crate::__private::dimension_key(id))
        };
        let id = screen.screen();
        if !Identifier::parse_namespaced(id).is_some_and(|id| id.namespace() != "minecraft") {
            log::warn!("[{owner}] Invalid dimension transition screen id: {id}");
        } else {
            let key = match screen.transition() {
                DimensionTransition::Incoming(to) => resolve(to).map(TransitionKey::Incoming),
                DimensionTransition::Outgoing(from) => resolve(from).map(TransitionKey::Outgoing),
                DimensionTransition::Conditional { to, from } => resolve(to)
                    .zip(resolve(from))
                    .map(|(to, from)| TransitionKey::Conditional { to, from }),
            };
            if let Some(key) = key {
                if !registrations.iter().any(|entry| entry.key == key) {
                    registrations.push(Self { key, owner, screen });
                    return true;
                }
                log::warn!("[{owner}] Duplicate dimension transition: {id}");
            } else {
                log::warn!("[{owner}] Invalid dimension transition: {id}");
            }
        }
        crate::runtime::discard_registration(REGISTRATION, Some(owner), screen);
        false
    }
}

impl RegisterDimensionTransitionScreenEvent<'_> {
    /// Stage a factory for this travel key. Returns `false` for an invalid ID or an
    /// occupied key, including one staged earlier in this callback; the first wins.
    /// `true` accepts the factory into this callback's batch. A panic or phase refusal
    /// discards that batch without reserving its keys for later listeners.
    pub fn register(&mut self, screen: DimensionTransitionScreen) -> bool {
        let owner =
            crate::content::crash::current_native_mod().expect("registration listener is marked");
        RegisteredTransitionScreen::register(self.screens, owner, screen)
    }
}

type TransitionFactory = dyn Fn(&dyn ClientHost, DimensionTransitionReason) -> Box<dyn DimensionTransitionPresentation>
    + Send
    + Sync
    + 'static;

// no wire: the registration owns a factory with process-local code and captures.
/// A loading-screen factory registered once by `Hooks::register_dimension_transition_screen`.
/// Only travel that opens a new loading screen can select it; login and death never select one.
// exhaustive: private storage keeps the travel key and its per-load factory together.
pub struct DimensionTransitionScreen {
    screen: String,
    transition: DimensionTransition,
    factory: Box<TransitionFactory>,
}

impl DimensionTransitionScreen {
    /// Register a factory for `transition` under a namespaced, non-vanilla screen ID.
    /// The ID is only used for validation and diagnostics. On a matching new load, the
    /// client calls the factory once as the registering mod and owns the returned presentation
    /// until the loading screen closes, even if the tracker restarts for another level.
    /// Capture per-load state in the returned presentation, not in the registration itself.
    pub fn new(
        screen: String,
        transition: DimensionTransition,
        factory: impl Fn(
            &dyn ClientHost,
            DimensionTransitionReason,
        ) -> Box<dyn DimensionTransitionPresentation>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            screen,
            transition,
            factory: Box::new(factory),
        }
    }

    /// The namespaced ID used for validation and diagnostics.
    pub fn screen(&self) -> &str {
        &self.screen
    }

    /// Which travel selects this factory.
    pub fn transition(&self) -> &DimensionTransition {
        &self.transition
    }

    /// Create the presentation for one new loading screen with its initial reason.
    pub fn create(
        &self,
        host: &dyn ClientHost,
        reason: DimensionTransitionReason,
    ) -> Box<dyn DimensionTransitionPresentation> {
        (self.factory)(host, reason)
    }
}

// no wire: each presentation is a process-local callback or object retained by one load.
/// The complete presentation of one loading screen, either an object or a draw closure.
/// It controls what is painted, not when the client finishes loading.
pub trait DimensionTransitionPresentation: Send + Sync + 'static {
    /// Draw under the registering mod's identity using this frame's reason and optional
    /// smoothed integrated-server progress. The tracker, not the presentation, decides when
    /// the screen closes. The reason can change while the same presentation remains active.
    fn draw(
        &self,
        host: &dyn ClientHost,
        reason: DimensionTransitionReason,
        progress: Option<f32>,
        draw: &mut dyn DrawContext,
    );
}

impl<F> DimensionTransitionPresentation for F
where
    F: Fn(&dyn ClientHost, DimensionTransitionReason, Option<f32>, &mut dyn DrawContext)
        + Send
        + Sync
        + 'static,
{
    fn draw(
        &self,
        host: &dyn ClientHost,
        reason: DimensionTransitionReason,
        progress: Option<f32>,
        draw: &mut dyn DrawContext,
    ) {
        self(host, reason, progress, draw);
    }
}

/// NeoForge's incoming, outgoing and conditional registration variants.
#[derive(Clone, Debug, PartialEq, Eq)]
// exhaustive: the three registration variants
#[derive(sunrise_mod_api_macros::Wire)]
pub enum DimensionTransition {
    /// Travel to this dimension (`registerIncomingEffect`).
    Incoming(String),
    /// Travel from this dimension (`registerOutgoingEffect`).
    Outgoing(String),
    /// Travel to `to` from `from` (`registerConditionalEffect`), highest priority.
    Conditional {
        /// Destination dimension id.
        to: String,
        /// Source dimension id.
        from: String,
    },
}

/// Why the client is displaying its level-loading screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// exhaustive: LevelLoadingScreen.Reason's three variants
#[derive(sunrise_mod_api_macros::Wire)]
pub enum DimensionTransitionReason {
    /// Travel involving the Nether, except a death respawn.
    NetherPortal,
    /// Travel involving the End, except a death respawn or Nether travel.
    EndPortal,
    /// Login, death respawn, or other travel.
    Other,
}

#[cfg(test)]
mod dimension_transition_tests {
    use crate::*;

    struct Client;
    impl Host for Client {}
    crate::impl_host_areas!(Client);
    impl ClientHost for Client {}
    crate::impl_client_host_areas!(Client);

    #[derive(Default)]
    struct Canvas(Vec<([f32; 4], u32)>);
    impl DrawContext for Canvas {
        fn size(&self) -> [f32; 2] {
            [320.0, 240.0]
        }
        fn fill(&mut self, rect: [f32; 4], color: u32) {
            self.0.push((rect, color));
        }
    }

    struct Arrival;
    #[crate::__subscriptions]
    impl Hooks for Arrival {
        fn register_dimension_transition_screen(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterDimensionTransitionScreenEvent<'_>,
        ) {
            event.register(DimensionTransitionScreen::new(
                "test:arrival".into(),
                DimensionTransition::Conditional {
                    to: "test:b".into(),
                    from: "test:a".into(),
                },
                |_, initial_reason| {
                    Box::new(
                        move |_: &dyn ClientHost, reason, progress, draw: &mut dyn DrawContext| {
                            assert_eq!(initial_reason, DimensionTransitionReason::Other);
                            assert_eq!(reason, DimensionTransitionReason::NetherPortal);
                            let [w, h] = draw.size();
                            draw.fill([0.0, 0.0, w, h], 0xFF102030);
                            if let Some(progress) = progress {
                                draw.fill([0.0, 230.0, w * progress, 10.0], 0xFF00FF00);
                            }
                        },
                    )
                },
            ));
        }
    }

    #[test]
    fn transition_registration_reports_duplicates_and_releases_refused_keys() {
        use crate::__private::{LoadAccess, ModPhase, PhaseOwner, check_access};
        use std::sync::{Arc, Mutex};

        struct Probe {
            results: Arc<Mutex<Vec<bool>>>,
            keys: &'static [&'static str],
            refuse: bool,
        }
        #[crate::__subscriptions]
        impl Hooks for Probe {
            fn register_dimension_transition_screen(
                &self,
                _: &RegistrationContext,
                event: &mut RegisterDimensionTransitionScreenEvent<'_>,
            ) {
                for &key in self.keys {
                    let accepted = event.register(DimensionTransitionScreen::new(
                        "example:loading".into(),
                        DimensionTransition::Incoming(key.into()),
                        |_, _| Box::new(|_: &dyn ClientHost, _, _, _: &mut dyn DrawContext| {}),
                    ));
                    self.results.lock().unwrap().push(accepted);
                }
                if self.refuse {
                    check_access("transition_world_only", LoadAccess::WorldOnly);
                }
            }
        }

        let results = Arc::new(Mutex::new(Vec::new()));
        const LISTENERS: [(&str, &[&str], bool); 3] = [
            ("first", &["example:a", "example:a"], false),
            ("refused", &["example:a", "example:b"], true),
            ("last", &["example:a", "example:b"], false),
        ];
        let mods = LISTENERS
            .into_iter()
            .map(|(id, keys, refuse)| {
                ModEntry::new(
                    id,
                    Box::new(Probe {
                        results: results.clone(),
                        keys,
                        refuse,
                    }),
                )
            })
            .collect::<Vec<_>>()
            .into();
        testing::with_dispatch(mods, || {
            let _phase = PhaseOwner::scope(ModPhase::Registration);
            let mut screens = Vec::new();
            dispatch_register_dimension_transition_screen(
                &RegistrationContext::new(),
                &mut screens,
            );
            assert_eq!(
                *results.lock().unwrap(),
                [true, false, false, true, false, true]
            );
            assert_eq!(
                screens
                    .iter()
                    .map(|screen| screen.owner)
                    .collect::<Vec<_>>(),
                ["first", "last"]
            );
        });
    }

    #[test]
    fn dimension_transition_factory_keeps_the_registrant_and_draws_with_live_data() {
        testing::with_dispatch(
            vec![ModEntry::new("test_registrant", Box::new(Arrival))].into(),
            || {
                let mut screens = Vec::new();
                dispatch_register_dimension_transition_screen(
                    &RegistrationContext::new(),
                    &mut screens,
                );
                let [screen] = screens.as_slice() else {
                    panic!("expected one registration");
                };
                assert_eq!(screen.owner, "test_registrant");
                assert_eq!(screen.screen.screen(), "test:arrival");
                assert_eq!(
                    screen.screen.transition(),
                    &DimensionTransition::Conditional {
                        to: "test:b".into(),
                        from: "test:a".into()
                    },
                );
                let presentation = screen
                    .screen
                    .create(&Client, DimensionTransitionReason::Other);
                let mut canvas = Canvas::default();
                presentation.draw(
                    &Client,
                    DimensionTransitionReason::NetherPortal,
                    Some(0.25),
                    &mut canvas,
                );
                assert_eq!(
                    canvas.0,
                    [
                        ([0.0, 0.0, 320.0, 240.0], 0xFF102030),
                        ([0.0, 230.0, 80.0, 10.0], 0xFF00FF00)
                    ]
                );
            },
        );
    }

    #[test]
    fn dimension_transition_without_listeners_collects_nothing() {
        testing::with_dispatch(Vec::new().into(), || {
            let mut screens = Vec::new();
            dispatch_register_dimension_transition_screen(
                &RegistrationContext::new(),
                &mut screens,
            );
            assert!(screens.is_empty());
        });
    }
}

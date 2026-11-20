use mod_api::{
    BroadcastMessageEvent, ClientHost, ClientTickPostEvent, Flow, Hooks, Host, HostTransaction,
    LogLevel, LogOutcome, ModLogger, Registrar, RegistrationContext, ServerHost,
    ServerStartedEvent, Text,
};
use std::sync::OnceLock;

#[derive(Default)]
struct Fixture {
    logger: OnceLock<ModLogger>,
    client_once: OnceLock<()>,
}

#[mod_api::sunrise_mod]
impl Hooks for Fixture {
    fn register(&self, _: &mut dyn Registrar, cx: &RegistrationContext) {
        let logger = cx.logger();
        assert!(logger.enabled(LogLevel::Info));
        assert_eq!(
            logger.log(
                LogLevel::Info,
                format_args!("mod-logger fixture: registered {}", 17)
            ),
            LogOutcome::Queued,
        );
        assert!(self.logger.set(logger).is_ok());
    }

    fn client_tick_post(&self, _: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
        self.client_once.get_or_init(|| {
            let logger = self.logger.get().unwrap();
            assert_eq!(
                logger.log(
                    LogLevel::Warn,
                    format_args!("mod-logger fixture: client first")
                ),
                LogOutcome::Queued,
            );
            assert_eq!(
                logger.log(
                    LogLevel::Error,
                    format_args!("mod-logger fixture: client second")
                ),
                LogOutcome::Queued,
            );
        });
    }

    fn server_started(&self, host: &mut dyn ServerHost, event: &mut ServerStartedEvent) {
        assert!(event.dedicated);
        let logger = self.logger.get().unwrap();
        let result: Result<(), ()> = host.transaction(|_, _| {
            assert_eq!(
                logger.log(
                    LogLevel::Warn,
                    format_args!("mod-logger fixture: before rollback")
                ),
                LogOutcome::Queued,
            );
            Err(())
        });
        assert_eq!(result, Err(()));
        assert_eq!(
            logger.log(
                LogLevel::Error,
                format_args!("mod-logger fixture: after rollback")
            ),
            LogOutcome::Queued,
        );
    }

    fn broadcast_message(&self, _: &dyn Host, event: &mut BroadcastMessageEvent<'_>) -> Flow {
        assert_ne!(
            *event.line,
            Text::literal("mod-logger fixture: before rollback")
        );
        Flow::Continue
    }
}

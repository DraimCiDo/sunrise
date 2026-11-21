mod api {
    #[derive(Clone, Copy)]
    pub enum BlockScreenEffect {}

    impl super::ExtractBlockScreenEffectEvent<'_> {
        pub fn set_cancelled(&mut self, cancelled: bool) {
            self.cancelled = cancelled;
        }
    }
}

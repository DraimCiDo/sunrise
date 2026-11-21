// no wire: dispatch fixture for a read-only view.
struct EntityView;

impl EntityView {
    fn kind(&self) -> Option<u32> {
        Some(0)
    }
}

impl EntityRef {
    fn with_view<H: ?Sized, R>(self, _: &H, read: impl FnOnce(&EntityView) -> R) -> Option<R> {
        Some(read(&EntityView))
    }
}

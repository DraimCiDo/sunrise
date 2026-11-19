#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenshotRequestError {
    Unavailable,
    Busy,
    InvalidSize,
    OutputUnavailable,
}

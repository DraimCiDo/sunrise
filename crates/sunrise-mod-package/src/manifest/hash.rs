//! stable-line: content-declarations
/// Checks a SHA-512 written as 128 lowercase hex digits, the form `[[jar]]` and `[runtime]` use.
pub(super) fn sha512_hex(value: &str) -> Result<(), &'static str> {
    if value.len() == 128
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err("must be 128 lowercase hex digits")
    }
}

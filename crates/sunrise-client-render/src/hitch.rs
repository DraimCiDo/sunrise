//! TEMPORARY hitch probe.
use std::time::Instant;
static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
pub fn now() -> f32 { T0.get_or_init(Instant::now).elapsed().as_secs_f32() * 1000.0 }

pub const MIN_MS: f32 = 4.0;
pub fn t<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
    let s = Instant::now();
    let r = f();
    let ms = s.elapsed().as_secs_f32() * 1000.0;
    if ms >= MIN_MS {
        eprintln!("HITCH @{:.0} {name} {ms:.1}ms", now());
    }
    r
}
pub fn bt(name: &'static str, s: Instant) {
    let ms = s.elapsed().as_secs_f32() * 1000.0;
    if ms >= 30.0 {
        eprintln!("HITCH @{:.0} {name} {ms:.1}ms\n{}", now(), std::backtrace::Backtrace::force_capture());
    } else if ms >= MIN_MS {
        eprintln!("HITCH @{:.0} {name} {ms:.1}ms", now());
    }
}

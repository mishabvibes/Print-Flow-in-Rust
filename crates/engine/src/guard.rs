//! The last line of defence: a panic in a command must not take the app (and the user's unsaved
//! work) down. It is a bug when it fires; `Result` is how failures are meant to travel.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs `f`; a panic becomes `Err(message)`.
pub fn catch_panic<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|p| {
        let msg =
            p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".into());
        log::error!("panic caught by guard: {msg}");
        msg
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::panic)]
    fn panics_become_errors() {
        assert_eq!(catch_panic(|| 3), Ok(3));
        assert_eq!(catch_panic(|| -> i32 { panic!("boom") }), Err("boom".to_string()));
    }
}

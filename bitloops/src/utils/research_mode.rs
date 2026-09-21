//! Archiver-only mode for the research build.
//!
//! When enabled, the CLI does the one thing the study needs — archive the code
//! an agent wrote at the end of each turn — and skips the rest of the Bitloops
//! pipeline: no daemon, no DuckDB, no checkpoints, no sync or ingest.
//!
//! This exists so participants have nothing to keep running and nothing that
//! can fail in a way they would have to debug. The full CLI is still there when
//! the variable is unset.

use std::env;

/// Set to a non-empty value to run in archiver-only mode.
pub const ARCHIVER_ONLY_ENV: &str = "CYCLOOPS_ARCHIVER_ONLY";

/// True when the CLI should archive turns and do nothing else.
pub fn archiver_only() -> bool {
    env::var(ARCHIVER_ONLY_ENV).is_ok_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::process_state::with_env_var;

    #[test]
    fn archiver_only_is_off_by_default() {
        with_env_var(ARCHIVER_ONLY_ENV, None, || {
            assert!(!archiver_only(), "full CLI must remain the default");
        });
    }

    #[test]
    fn archiver_only_turns_on_for_any_non_empty_value() {
        with_env_var(ARCHIVER_ONLY_ENV, Some("1"), || {
            assert!(archiver_only());
        });
    }

    #[test]
    fn archiver_only_ignores_blank_values() {
        with_env_var(ARCHIVER_ONLY_ENV, Some("   "), || {
            assert!(
                !archiver_only(),
                "a blank value is an unset variable, not an opt-in"
            );
        });
    }
}

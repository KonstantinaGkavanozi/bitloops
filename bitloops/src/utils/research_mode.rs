//! Archiver-only mode for the research build.
//!
//! This build archives the code an agent wrote at the end of each turn and
//! skips the rest of the Bitloops pipeline: no daemon, no DuckDB, no
//! checkpoints, no sync or ingest.
//!
//! That is the DEFAULT, not an opt-in. Making it depend on an environment
//! variable meant a source build, a hand-copied binary, or a shell that had
//! not picked up the installer's profile edit all silently got the full
//! pipeline — prompting for embeddings and sync, and expecting a daemon
//! nobody had started. The safe behaviour has to be the one you get when
//! nothing is configured.
//!
//! Set `CYCLOOPS_FULL_CLI` to a non-empty value for the complete CLI.

use std::env;

/// Set to a non-empty value to run the full Bitloops pipeline instead.
pub const FULL_CLI_ENV: &str = "CYCLOOPS_FULL_CLI";

/// The single gate every archiver-only decision goes through.
pub fn archiver_only() -> bool {
    !env::var(FULL_CLI_ENV).is_ok_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::process_state::with_env_var;

    #[test]
    fn archiver_only_is_the_default() {
        with_env_var(FULL_CLI_ENV, None, || {
            assert!(
                archiver_only(),
                "an unconfigured install must archive and nothing else"
            );
        });
    }

    #[test]
    fn full_cli_opts_out() {
        with_env_var(FULL_CLI_ENV, Some("1"), || {
            assert!(!archiver_only());
        });
    }

    #[test]
    fn blank_value_is_not_an_opt_out() {
        with_env_var(FULL_CLI_ENV, Some("   "), || {
            assert!(
                archiver_only(),
                "a blank value is an unset variable, not a request for full mode"
            );
        });
    }
}

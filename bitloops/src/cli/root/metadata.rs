pub const ROOT_NAME: &str = "cycloops";
pub const ROOT_SHORT_ABOUT: &str = "Cycloops CLI";
pub const ROOT_LONG_ABOUT: &str = r#"Cycloops CLI - a research build of Bitloops that archives agent-written code

Getting Started:
  Run 'cycloops init' inside a repository to install the agent hooks.
  Archiving then runs on its own; in archiver-only mode there is no daemon
  to start. For more information, see:
  https://github.com/KonstantinaGkavanozi/bitloops

Environment Variables:
  ACCESSIBLE    Set to any value (e.g., ACCESSIBLE=1) to enable accessibility
                mode. This uses simpler text prompts instead of interactive
                TUI elements, which works better with screen readers.
"#;

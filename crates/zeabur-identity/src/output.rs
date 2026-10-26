//! The only place that writes to stdout and stderr directly.
//!
//! Server diagnostics go through `tracing`. CLI subcommands print their results here, so that a
//! review of this module covers every byte the binary writes outside structured logs.

/// Writes one line of command output to stdout.
#[expect(clippy::print_stdout, reason = "the audited CLI output path")]
pub fn stdout(line: &str) {
    println!("{line}");
}

/// Writes one line of user-facing error text to stderr.
#[expect(clippy::print_stderr, reason = "the audited CLI output path")]
pub fn stderr(line: &str) {
    eprintln!("{line}");
}

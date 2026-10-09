//! The streaming extension keywords and the language-neutral facts a generated
//! caller reads from them.
//!
//! The keywords are declared here so the loader that validates them and the
//! emitters that honor them share one vocabulary. Nothing here holds
//! target-language policy.
//!
//! See `specs/json-schema/long-poll.md` for the authoritative rules.

/// Marks an `operations:` entry as long-pollable.
pub const LONG_POLL_KEYWORD: &str = "x-nexus-long-poll";

/// The `x-nexus-long-poll` member naming the input's wait-hint member.
pub const LONG_POLL_WAIT_MEMBER: &str = "wait-field";

/// The `x-nexus-long-poll` member naming the output's emptiness-test member.
pub const LONG_POLL_RESULT_MEMBER: &str = "result-field";

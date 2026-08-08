//! Strict input specification for the internal Phase 0B generic rehearsal.
//!
//! This crate parses and authenticates rehearsal inputs. It deliberately does
//! not execute native or browser hosts, compare frames, publish evidence, or
//! decide the authoritative Phase 0B gate.

mod spec;

pub use spec::{CaseError, CaseManifest, LoadedCase, load_case, parse_case};

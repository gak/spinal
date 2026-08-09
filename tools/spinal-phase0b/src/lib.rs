//! Foundations for the internal Phase 0B generic rehearsal.
//!
//! This crate authenticates rehearsal inputs, captures the fixed native semantic
//! and event schedules, and compares complete semantic frames, event windows,
//! and fixed-profile PNGs. Its public fixed contract is also consumed by
//! `spinal-app`'s opt-in browser observation path. That path captures fresh
//! Current/Proposed event windows before a nonce-bound protocol pairs eight
//! isolated presentations and original browser PNGs with the exact runtime
//! identities and semantic generations that produced them. This crate does not
//! provide the full two-host owner runner, independent references, build/browser
//! provenance, evidence publication, or the authoritative Phase 0B decision.

pub mod browser_capture;
pub mod browser_observation;
pub mod bundle;
pub mod capture;
pub mod contract;
pub mod event_capture;
pub mod event_compare;
pub mod pixel_compare;
pub mod semantic_compare;
mod spec;

pub use bundle::{
    CaseBundleLoadError, CaseBundleSide, LoadedCaseRuntimeBundles, load_case_runtime_bundles,
};
pub use spec::{
    CaseError, CaseManifest, LoadedCase, SemanticExecutionPlan, SemanticSampleInputs, load_case,
    parse_case,
};

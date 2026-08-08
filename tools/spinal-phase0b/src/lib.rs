//! Foundations for the internal Phase 0B generic rehearsal.
//!
//! This crate authenticates rehearsal inputs, compares complete semantic
//! frames, and can capture the fixed native sample schedule. Its public fixed
//! contract is also consumed by `spinal-app`'s opt-in browser observation path,
//! which emits semantic observations bound to Current and Proposed runtime
//! identities. This crate does not provide the full two-host owner runner,
//! compare pixels or events, publish evidence, or decide the authoritative
//! Phase 0B gate.

pub mod bundle;
pub mod capture;
pub mod contract;
pub mod semantic_compare;
mod spec;

pub use bundle::{
    CaseBundleLoadError, CaseBundleSide, LoadedCaseRuntimeBundles, load_case_runtime_bundles,
};
pub use spec::{
    CaseError, CaseManifest, LoadedCase, SemanticExecutionPlan, SemanticSampleInputs, load_case,
    parse_case,
};

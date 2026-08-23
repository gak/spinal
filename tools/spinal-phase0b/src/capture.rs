//! Bounded native semantic capture for the fixed Phase 0B rehearsal schedule.
//!
//! This module observes renderer-neutral frames only. It does not collect or
//! compare authored events, browser output, or pixels, and its output never
//! constitutes a rehearsal result or a Phase 0 gate decision.

use std::{array, sync::Arc, time::Duration};

use bevy::{
    asset::{AssetPlugin, Assets, Handle},
    image::Image,
    prelude::{App, Entity, MinimalPlugins},
};
use bevy_spinal::{
    SpinalAnimator, SpinalAsset, SpinalAssetLoaderError, SpinalAtlasPage, SpinalInstance,
    SpinalPlugin, SpinalSemanticCapture, SpinalSkinLayers,
    spinal::{PlaybackMode, SemanticFrame, SkeletonAsset, Transition, ValidatedRuntimeBundle},
};
use thiserror::Error;

use crate::{
    LoadedCaseRuntimeBundles,
    contract::{ALTERNATE_SKIN_NAME, ANIMATION_DURATION},
};

/// Maximum Bevy updates allowed to publish one requested sample.
///
/// This is an execution bound, not a timing allowance. Playback is paused and
/// every pose is requested with an absolute seek.
pub const MAX_UPDATES_PER_SAMPLE: usize = 8;

pub use crate::contract::{
    ANIMATION_NAME as NATIVE_ANIMATION_NAME, SAMPLE_COUNT as NATIVE_SAMPLE_COUNT,
    SAMPLE_SCHEDULE as NATIVE_SAMPLE_SCHEDULE, Sample as NativeSample,
};

/// One side of the fixed Current-versus-Proposed native observation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NativeSource {
    /// The immutable Current runtime bundle.
    Current,
    /// The immutable Proposed runtime bundle.
    Proposed,
}

impl NativeSource {
    const ALL: [Self; 2] = [Self::Current, Self::Proposed];

    const fn index(self) -> usize {
        match self {
            Self::Current => 0,
            Self::Proposed => 1,
        }
    }
}

/// One accepted renderer-neutral observation.
///
/// Acceptance records the exact command generations acknowledged by the
/// runtime. The frame is an observation only; no comparison or pass judgment
/// is made here.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeSemanticObservation {
    source: NativeSource,
    sample: NativeSample,
    frame_revision: u64,
    acknowledged_play_revision: u64,
    acknowledged_seek_revision: u64,
    frame: SemanticFrame,
}

impl NativeSemanticObservation {
    /// Returns which immutable source produced this frame.
    #[must_use]
    pub const fn source(&self) -> NativeSource {
        self.source
    }

    /// Returns which fixed schedule entry produced this frame.
    #[must_use]
    pub const fn sample(&self) -> NativeSample {
        self.sample
    }

    /// Returns the successful-capture generation accepted by the harness.
    #[must_use]
    pub const fn frame_revision(&self) -> u64 {
        self.frame_revision
    }

    /// Returns the freshly applied `Once` play-command generation.
    #[must_use]
    pub const fn acknowledged_play_revision(&self) -> u64 {
        self.acknowledged_play_revision
    }

    /// Returns the freshly applied absolute-seek generation.
    #[must_use]
    pub const fn acknowledged_seek_revision(&self) -> u64 {
        self.acknowledged_seek_revision
    }

    /// Returns the complete owned semantic frame.
    #[must_use]
    pub const fn frame(&self) -> &SemanticFrame {
        &self.frame
    }
}

/// The fixed, bounded native semantic observations for both sources.
///
/// Array order is exactly [`NATIVE_SAMPLE_SCHEDULE`]. These observations are
/// inputs to later independent comparison; their existence is not a pass.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeSemanticObservations {
    current: [NativeSemanticObservation; NATIVE_SAMPLE_COUNT],
    proposed: [NativeSemanticObservation; NATIVE_SAMPLE_COUNT],
}

impl NativeSemanticObservations {
    /// Returns Current observations in fixed schedule order.
    #[must_use]
    pub const fn current(&self) -> &[NativeSemanticObservation; NATIVE_SAMPLE_COUNT] {
        &self.current
    }

    /// Returns Proposed observations in fixed schedule order.
    #[must_use]
    pub const fn proposed(&self) -> &[NativeSemanticObservation; NATIVE_SAMPLE_COUNT] {
        &self.proposed
    }
}

/// Gate eligibility of the identity-bound native capture seam.
///
/// This seam captures renderer-neutral native observations only. It does not
/// compare them with an independent reference or collect the browser, event,
/// and pixel evidence required by Phase 0B, so it can never satisfy a gate.
pub const IDENTITY_BOUND_NATIVE_CAPTURE_GATE_ELIGIBLE: bool = false;

/// Exact immutable identity of one validated runtime bundle used for capture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRuntimeBundleIdentity {
    manifest_sha256: Box<str>,
    content_sha256: Box<str>,
}

impl NativeRuntimeBundleIdentity {
    fn from_bundle(bundle: &ValidatedRuntimeBundle) -> Self {
        Self {
            manifest_sha256: bundle.manifest_sha256().into(),
            content_sha256: bundle.content_sha256().into(),
        }
    }

    /// Returns the SHA-256 of the exact retained runtime-manifest bytes.
    #[must_use]
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    /// Returns the deterministic SHA-256 of normalized paths and exact bytes.
    #[must_use]
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }
}

/// Identity-bound native observations from one immutable Current/Proposed pair.
///
/// The source-specific identities and observations are produced in the same
/// bounded call. This prevents an unlabelled pair of Bevy handles from being
/// mistaken for observations of a particular validated bundle pair. The result remains
/// a gate-ineligible primitive, not rehearsal evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct IdentityBoundNativeCapture {
    observations: NativeSemanticObservations,
    current_identity: NativeRuntimeBundleIdentity,
    proposed_identity: NativeRuntimeBundleIdentity,
}

impl IdentityBoundNativeCapture {
    /// Returns all fixed native observations in source and schedule order.
    #[must_use]
    pub const fn observations(&self) -> &NativeSemanticObservations {
        &self.observations
    }

    /// Returns the exact identity of the retained Current bundle.
    #[must_use]
    pub const fn current_identity(&self) -> &NativeRuntimeBundleIdentity {
        &self.current_identity
    }

    /// Returns the exact identity of the retained Proposed bundle.
    #[must_use]
    pub const fn proposed_identity(&self) -> &NativeRuntimeBundleIdentity {
        &self.proposed_identity
    }

    /// Returns whether this capture alone may satisfy a Phase 0 gate.
    ///
    /// This is unconditionally `false`; eligibility is not input data and
    /// cannot be promoted by a caller.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        IDENTITY_BOUND_NATIVE_CAPTURE_GATE_ELIGIBLE
    }
}

/// Failure while constructing or executing an identity-bound native capture.
#[derive(Debug, Error)]
pub enum IdentityBoundNativeCaptureError {
    /// A retained core asset could not be paired with its in-memory page handles.
    #[error("could not construct the {capture_source:?} capture asset: {source}")]
    AssetConstruction {
        /// Immutable side whose Bevy adapter construction failed.
        capture_source: NativeSource,
        /// Typed Bevy adapter construction failure.
        #[source]
        source: SpinalAssetLoaderError,
    },
    /// The fixed bounded schedule could not be captured.
    #[error(transparent)]
    Capture(#[from] NativeCaptureError),
}

/// Why a source has not yet published an acceptable requested frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapturePendingReason {
    /// No owned semantic frame is currently available.
    MissingFrame,
    /// The frame is not newer than the pre-command baseline.
    FrameRevisionNotAdvanced {
        /// Revision observed before issuing the sample command.
        baseline: u64,
        /// Latest revision published by the runtime.
        observed: u64,
    },
    /// The frame did not acknowledge the freshly issued play command.
    PlayRevisionMismatch {
        /// Fresh play generation issued for this sample.
        expected: u64,
        /// Play generation acknowledged by the observed frame.
        observed: Option<u64>,
    },
    /// The frame did not acknowledge the freshly issued absolute seek.
    SeekRevisionMismatch {
        /// Fresh seek generation issued for this sample.
        expected: u64,
        /// Seek generation acknowledged by the observed frame.
        observed: Option<u64>,
    },
    /// The solved frame did not contain the exact requested skin layers.
    SkinLayersMismatch {
        /// Complete ordered selection requested for this sample.
        expected: Vec<Box<str>>,
        /// Complete ordered selection captured in the frame.
        observed: Vec<Box<str>>,
    },
}

/// Failure to execute the bounded native semantic schedule.
#[derive(Debug, Error, PartialEq)]
pub enum NativeCaptureError {
    /// The application has no Bevy storage for Spinal assets.
    #[error("the capture application has no `Assets<SpinalAsset>` resource")]
    MissingAssetStorage,
    /// A supplied asset handle was not present in Bevy's asset storage.
    #[error("the {capture_source:?} capture asset is not loaded")]
    AssetNotLoaded {
        /// Source whose handle did not resolve.
        capture_source: NativeSource,
    },
    /// The fixed animation was absent from one supplied asset.
    #[error("the {capture_source:?} capture asset has no `{animation}` animation")]
    MissingAnimation {
        /// Source whose asset failed validation.
        capture_source: NativeSource,
        /// Fixed version-one animation name.
        animation: &'static str,
    },
    /// The fixed animation did not have its exact required duration.
    #[error(
        "the {capture_source:?} `{animation}` animation has duration {actual:?}; expected exactly {expected:?}"
    )]
    AnimationDurationMismatch {
        /// Source whose asset failed validation.
        capture_source: NativeSource,
        /// Fixed version-one animation name.
        animation: &'static str,
        /// Exact version-one duration.
        expected: Duration,
        /// Authored duration found in the asset.
        actual: Duration,
    },
    /// The alternate attachment-only skin was absent from one supplied asset.
    #[error("the {capture_source:?} capture asset has no `{skin}` skin")]
    MissingSkin {
        /// Source whose asset failed validation.
        capture_source: NativeSource,
        /// Fixed version-one alternate skin name.
        skin: &'static str,
    },
    /// An entity owned by the capture harness disappeared during execution.
    #[error("the {capture_source:?} capture entity disappeared")]
    EntityDisappeared {
        /// Source whose entity disappeared.
        capture_source: NativeSource,
    },
    /// A required public runtime component was removed during execution.
    #[error("the {capture_source:?} capture entity is missing `{component}`")]
    MissingRequiredComponent {
        /// Source whose entity is incomplete.
        capture_source: NativeSource,
        /// Stable public component name.
        component: &'static str,
    },
    /// At least one source did not publish an acceptable frame within the
    /// fixed update bound.
    #[error(
        "sample {sample:?} was not observed within {updates} updates (Current: {current:?}; Proposed: {proposed:?})"
    )]
    ObservationTimedOut {
        /// Fixed schedule entry that timed out.
        sample: NativeSample,
        /// Exact number of attempted Bevy updates.
        updates: usize,
        /// Last Current rejection, or `None` if Current was accepted.
        current: Option<CapturePendingReason>,
        /// Last Proposed rejection, or `None` if Proposed was accepted.
        proposed: Option<CapturePendingReason>,
    },
}

/// Creates the renderer-free Bevy application expected by
/// [`capture_native_schedule`].
///
/// Runtime assets still need to be loaded or inserted by the caller before
/// capture. `bevy_spinal` must be linked without its `render` default feature
/// for this helper to remain renderer-free.
pub fn new_headless_capture_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), SpinalPlugin));
    app
}

/// Captures the fixed native schedule directly from one loaded case bundle pair.
///
/// Each Bevy [`SpinalAsset`] receives the exact retained core asset from its
/// corresponding [`ValidatedRuntimeBundle`] plus one source-ordered in-memory
/// image handle per retained atlas page. The returned identities are copied
/// from those same two validated bundles before the handles are passed to
/// [`capture_native_schedule`].
///
/// This is an identity-bound native primitive only. It does not compare the
/// observations or produce Phase 0B evidence; [`IdentityBoundNativeCapture::gate_eligible`]
/// is therefore always `false`.
pub fn capture_loaded_case_runtime_bundles(
    bundles: &LoadedCaseRuntimeBundles,
) -> Result<IdentityBoundNativeCapture, IdentityBoundNativeCaptureError> {
    let current_identity = NativeRuntimeBundleIdentity::from_bundle(bundles.current());
    let proposed_identity = NativeRuntimeBundleIdentity::from_bundle(bundles.proposed());
    let mut app = new_headless_capture_app();
    let current = insert_bundle_capture_asset(&mut app, NativeSource::Current, bundles.current())?;
    let proposed =
        insert_bundle_capture_asset(&mut app, NativeSource::Proposed, bundles.proposed())?;
    let observations = capture_native_schedule(&mut app, current, proposed)?;

    Ok(IdentityBoundNativeCapture {
        observations,
        current_identity,
        proposed_identity,
    })
}

fn insert_bundle_capture_asset(
    app: &mut App,
    source: NativeSource,
    bundle: &ValidatedRuntimeBundle,
) -> Result<Handle<SpinalAsset>, IdentityBoundNativeCaptureError> {
    let pages = bundle
        .asset()
        .atlas_pages()
        .map(|page| {
            let image = app
                .world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::default());
            SpinalAtlasPage::new(page.name(), image)
        })
        .collect();
    insert_capture_asset(app, source, Arc::clone(bundle.asset()), pages)
}

fn insert_capture_asset(
    app: &mut App,
    source: NativeSource,
    skeleton: Arc<SkeletonAsset>,
    pages: Vec<SpinalAtlasPage>,
) -> Result<Handle<SpinalAsset>, IdentityBoundNativeCaptureError> {
    let asset = SpinalAsset::new(skeleton, pages).map_err(|source_error| {
        IdentityBoundNativeCaptureError::AssetConstruction {
            capture_source: source,
            source: source_error,
        }
    })?;
    Ok(app
        .world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .add(asset))
}

/// Executes the fixed native semantic schedule for preloaded Current and
/// Proposed assets.
///
/// The function creates two temporary opt-in capture entities, applies a fresh
/// `sway`/`Once` command for every schedule entry, pauses playback, applies the
/// entry's exact skin layers, and issues an absolute seek. A frame is retained
/// only when its capture revision advanced past the pre-command baseline and
/// it acknowledges both fresh command revisions and the exact skin selection.
/// Each sample receives at most [`MAX_UPDATES_PER_SAMPLE`] Bevy updates.
///
/// The caller must have added [`SpinalPlugin`] after Bevy's [`AssetPlugin`]
/// and must pass loaded [`SpinalAsset`] handles. Prefer
/// [`new_headless_capture_app`] when no other app configuration is needed.
///
/// This low-level helper does not prove that the handles were constructed from
/// a particular [`crate::LoadedCaseRuntimeBundles`] pair, so its observations
/// cannot be treated as rehearsal evidence. A later runner must construct the
/// two Bevy assets from those retained bundle bytes and preserve both bundle
/// content identities in the same bounded operation before calling capture.
pub fn capture_native_schedule(
    app: &mut App,
    current: Handle<SpinalAsset>,
    proposed: Handle<SpinalAsset>,
) -> Result<NativeSemanticObservations, NativeCaptureError> {
    validate_capture_asset(app, NativeSource::Current, &current)?;
    validate_capture_asset(app, NativeSource::Proposed, &proposed)?;

    let entities = [spawn_source(app, current), spawn_source(app, proposed)];
    let result = capture_entities(app, entities);

    for entity in entities {
        let _ = app.world_mut().despawn(entity);
    }

    result
}

fn validate_capture_asset(
    app: &App,
    source: NativeSource,
    handle: &Handle<SpinalAsset>,
) -> Result<(), NativeCaptureError> {
    let assets = app
        .world()
        .get_resource::<Assets<SpinalAsset>>()
        .ok_or(NativeCaptureError::MissingAssetStorage)?;
    let asset = assets
        .get(handle)
        .ok_or(NativeCaptureError::AssetNotLoaded {
            capture_source: source,
        })?;
    let skeleton = asset.skeleton();
    let animation = skeleton.animation_id(NATIVE_ANIMATION_NAME).ok_or(
        NativeCaptureError::MissingAnimation {
            capture_source: source,
            animation: NATIVE_ANIMATION_NAME,
        },
    )?;
    let actual = skeleton
        .animation(animation)
        .expect("an ID resolved from this skeleton must remain asset-scoped")
        .duration();
    if actual != ANIMATION_DURATION {
        return Err(NativeCaptureError::AnimationDurationMismatch {
            capture_source: source,
            animation: NATIVE_ANIMATION_NAME,
            expected: ANIMATION_DURATION,
            actual,
        });
    }
    if skeleton.skin_id(ALTERNATE_SKIN_NAME).is_none() {
        return Err(NativeCaptureError::MissingSkin {
            capture_source: source,
            skin: ALTERNATE_SKIN_NAME,
        });
    }
    Ok(())
}

fn spawn_source(app: &mut App, asset: Handle<SpinalAsset>) -> Entity {
    app.world_mut()
        .spawn((
            SpinalInstance::new(asset),
            SpinalAnimator::default(),
            SpinalSkinLayers::default(),
            SpinalSemanticCapture::default(),
        ))
        .id()
}

fn capture_entities(
    app: &mut App,
    entities: [Entity; 2],
) -> Result<NativeSemanticObservations, NativeCaptureError> {
    let mut observations: [Vec<NativeSemanticObservation>; 2] =
        array::from_fn(|_| Vec::with_capacity(NATIVE_SAMPLE_COUNT));

    for sample in NATIVE_SAMPLE_SCHEDULE {
        let expectations = [
            issue_sample(app, NativeSource::Current, entities[0], sample)?,
            issue_sample(app, NativeSource::Proposed, entities[1], sample)?,
        ];
        let mut accepted: [Option<NativeSemanticObservation>; 2] = array::from_fn(|_| None);
        let mut pending: [Option<CapturePendingReason>; 2] =
            array::from_fn(|_| Some(CapturePendingReason::MissingFrame));

        for _ in 0..MAX_UPDATES_PER_SAMPLE {
            app.update();

            for source in NativeSource::ALL {
                let index = source.index();
                if accepted[index].is_some() {
                    continue;
                }
                match observe_source(app, source, entities[index], sample, expectations[index])? {
                    Ok(observation) => {
                        accepted[index] = Some(observation);
                        pending[index] = None;
                    }
                    Err(reason) => pending[index] = Some(reason),
                }
            }

            if accepted.iter().all(Option::is_some) {
                break;
            }
        }

        if accepted.iter().any(Option::is_none) {
            return Err(NativeCaptureError::ObservationTimedOut {
                sample,
                updates: MAX_UPDATES_PER_SAMPLE,
                current: pending[0].take(),
                proposed: pending[1].take(),
            });
        }

        for source in NativeSource::ALL {
            let index = source.index();
            observations[index].push(
                accepted[index]
                    .take()
                    .expect("both source observations were checked above"),
            );
        }
    }

    let [current, proposed] = observations;
    Ok(NativeSemanticObservations {
        current: current
            .try_into()
            .expect("the closed schedule emits exactly four Current observations"),
        proposed: proposed
            .try_into()
            .expect("the closed schedule emits exactly four Proposed observations"),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CaptureExpectation {
    baseline_frame_revision: u64,
    play_revision: u64,
    seek_revision: u64,
}

fn issue_sample(
    app: &mut App,
    source: NativeSource,
    entity: Entity,
    sample: NativeSample,
) -> Result<CaptureExpectation, NativeCaptureError> {
    let baseline_frame_revision = app
        .world()
        .get_entity(entity)
        .map_err(|_| NativeCaptureError::EntityDisappeared {
            capture_source: source,
        })?
        .get::<SpinalSemanticCapture>()
        .ok_or(NativeCaptureError::MissingRequiredComponent {
            capture_source: source,
            component: "SpinalSemanticCapture",
        })?
        .frame_revision();

    let mut entity_mut = app.world_mut().get_entity_mut(entity).map_err(|_| {
        NativeCaptureError::EntityDisappeared {
            capture_source: source,
        }
    })?;
    entity_mut
        .get_mut::<SpinalSkinLayers>()
        .ok_or(NativeCaptureError::MissingRequiredComponent {
            capture_source: source,
            component: "SpinalSkinLayers",
        })?
        .set(sample.skin_layers().iter().copied());

    let mut animator = entity_mut.get_mut::<SpinalAnimator>().ok_or(
        NativeCaptureError::MissingRequiredComponent {
            capture_source: source,
            component: "SpinalAnimator",
        },
    )?;
    animator.play(
        NATIVE_ANIMATION_NAME,
        PlaybackMode::Once,
        Transition::Immediate,
    );
    animator.set_paused(true);
    animator.seek_to(sample.time());

    Ok(CaptureExpectation {
        baseline_frame_revision,
        play_revision: animator.revision(),
        seek_revision: animator.seek_revision(),
    })
}

fn observe_source(
    app: &App,
    source: NativeSource,
    entity: Entity,
    sample: NativeSample,
    expected: CaptureExpectation,
) -> Result<Result<NativeSemanticObservation, CapturePendingReason>, NativeCaptureError> {
    let capture = app
        .world()
        .get_entity(entity)
        .map_err(|_| NativeCaptureError::EntityDisappeared {
            capture_source: source,
        })?
        .get::<SpinalSemanticCapture>()
        .ok_or(NativeCaptureError::MissingRequiredComponent {
            capture_source: source,
            component: "SpinalSemanticCapture",
        })?;
    let frame = capture.frame();
    let actual_skin_layers = frame.map(|frame| frame.skin_layers().collect::<Vec<_>>());
    let state = ObservedCaptureState {
        frame_present: frame.is_some(),
        frame_revision: capture.frame_revision(),
        acknowledged_play_revision: capture.acknowledged_play_revision(),
        acknowledged_seek_revision: capture.acknowledged_seek_revision(),
        skin_layers: actual_skin_layers.as_deref().unwrap_or_default(),
    };

    if let Err(reason) = classify_observation(expected, sample.skin_layers(), state) {
        return Ok(Err(reason));
    }

    Ok(Ok(NativeSemanticObservation {
        source,
        sample,
        frame_revision: capture.frame_revision(),
        acknowledged_play_revision: expected.play_revision,
        acknowledged_seek_revision: expected.seek_revision,
        frame: frame
            .expect("classification requires a present frame")
            .clone(),
    }))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObservedCaptureState<'a> {
    frame_present: bool,
    frame_revision: u64,
    acknowledged_play_revision: Option<u64>,
    acknowledged_seek_revision: Option<u64>,
    skin_layers: &'a [&'a str],
}

fn classify_observation(
    expected: CaptureExpectation,
    expected_skin_layers: &[&str],
    observed: ObservedCaptureState<'_>,
) -> Result<(), CapturePendingReason> {
    if !observed.frame_present {
        return Err(CapturePendingReason::MissingFrame);
    }
    if observed.frame_revision <= expected.baseline_frame_revision {
        return Err(CapturePendingReason::FrameRevisionNotAdvanced {
            baseline: expected.baseline_frame_revision,
            observed: observed.frame_revision,
        });
    }
    if observed.acknowledged_play_revision != Some(expected.play_revision) {
        return Err(CapturePendingReason::PlayRevisionMismatch {
            expected: expected.play_revision,
            observed: observed.acknowledged_play_revision,
        });
    }
    if observed.acknowledged_seek_revision != Some(expected.seek_revision) {
        return Err(CapturePendingReason::SeekRevisionMismatch {
            expected: expected.seek_revision,
            observed: observed.acknowledged_seek_revision,
        });
    }
    if observed.skin_layers != expected_skin_layers {
        return Err(CapturePendingReason::SkinLayersMismatch {
            expected: expected_skin_layers
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
            observed: observed
                .skin_layers
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use bevy::{asset::Assets, image::Image};
    use bevy_spinal::{
        SpinalAtlasPage,
        spinal::{RuntimeBundleManifest, load_json},
    };
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::{load_case, load_case_runtime_bundles};

    const SMOKE_ATLAS: &[u8] = b"cat.png\n\tsize: 1, 1\nbody\n\tbounds: 0, 0, 1, 1\n";
    const SMOKE_JSON: &[u8] = br#"{
      "skeleton": { "spine": "4.3.23" },
      "bones": [{ "name": "root" }],
      "slots": [{ "name": "body", "bone": "root", "attachment": "body" }],
      "skins": [
        {
          "name": "default",
          "attachments": {
            "body": { "body": { "width": 32, "height": 32 } }
          }
        },
        {
          "name": "alternate",
          "attachments": {
            "body": { "body": { "width": 40, "height": 32 } }
          }
        }
      ],
      "animations": {
        "sway": {
          "bones": {
            "root": {
              "translate": [
                { "x": 0, "y": 0 },
                { "time": 1, "x": 10, "y": 0 }
              ]
            }
          }
        }
      }
    }"#;

    const CASE: &str = include_str!("../cases/generic-bevy-0.18.1.toml");
    const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 248, 255, 31,
        0, 3, 2, 1, 255, 230, 119, 11, 174, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    const EXPECTED: CaptureExpectation = CaptureExpectation {
        baseline_frame_revision: 4,
        play_revision: 7,
        seek_revision: 9,
    };

    struct LoadedBundlesFixture {
        _directory: tempfile::TempDir,
        case_path: PathBuf,
    }

    impl LoadedBundlesFixture {
        fn new(proposed_json: &[u8]) -> Self {
            let directory = tempfile::tempdir().expect("temporary loaded-bundle fixture");
            let current_manifest = write_runtime_bundle(
                directory.path(),
                "current/runtime.json",
                "Current capture fixture",
                SMOKE_JSON,
            );
            let proposed_manifest = write_runtime_bundle(
                directory.path(),
                "proposed/runtime.json",
                "Proposed capture fixture",
                proposed_json,
            );
            let case_path = directory.path().join("case.toml");
            let mut case = provide_runtime_manifest(
                CASE.to_owned(),
                "runtime_manifest = { required = true }",
                directory.path(),
                &current_manifest,
            );
            case = provide_runtime_manifest(
                case,
                "runtime_manifest = {required = true}",
                directory.path(),
                &proposed_manifest,
            );
            fs::write(&case_path, case).expect("write capture case");
            Self {
                _directory: directory,
                case_path,
            }
        }

        fn load(&self) -> LoadedCaseRuntimeBundles {
            let case = load_case(&self.case_path).expect("authenticate capture case");
            load_case_runtime_bundles(&case).expect("load immutable capture bundles")
        }
    }

    fn write_runtime_bundle(
        root: &Path,
        manifest_relative: &str,
        label: &str,
        json: &[u8],
    ) -> PathBuf {
        let manifest_path = root.join(manifest_relative);
        let manifest_directory = manifest_path.parent().expect("manifest parent");
        let files = BTreeMap::from([
            (PathBuf::from("rig/cat.json"), json.to_vec()),
            (PathBuf::from("rig/cat.atlas"), SMOKE_ATLAS.to_vec()),
            (PathBuf::from("rig/cat.png"), PNG.to_vec()),
        ]);
        let manifest = RuntimeBundleManifest::build(
            label,
            Path::new("rig/cat.json"),
            Path::new("rig/cat.atlas"),
            files.clone(),
        )
        .expect("build strict capture bundle")
        .0;
        fs::create_dir_all(manifest_directory).expect("create manifest directory");
        fs::write(&manifest_path, manifest).expect("write runtime manifest");
        for (relative, bytes) in files {
            let path = manifest_directory.join(relative);
            fs::create_dir_all(path.parent().expect("runtime file parent"))
                .expect("create runtime directory");
            fs::write(path, bytes).expect("write runtime file");
        }
        manifest_path
    }

    fn provide_runtime_manifest(
        case: String,
        placeholder: &str,
        root: &Path,
        manifest_path: &Path,
    ) -> String {
        assert_eq!(case.matches(placeholder).count(), 1, "unique manifest slot");
        let relative = manifest_path
            .strip_prefix(root)
            .expect("manifest remains under fixture root")
            .to_str()
            .expect("portable manifest path");
        let bytes = fs::read(manifest_path).expect("read runtime manifest");
        let field = placeholder.split_once('=').expect("manifest slot field").0;
        let reference = format!(
            "{field}= {{ required = true, path = \"{relative}\", byte_length = {}, sha256 = \"{}\" }}",
            bytes.len(),
            sha256_hex(&bytes),
        );
        case.replacen(placeholder, &reference, 1)
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn observed<'a>(skin_layers: &'a [&'a str]) -> ObservedCaptureState<'a> {
        ObservedCaptureState {
            frame_present: true,
            frame_revision: 5,
            acknowledged_play_revision: Some(7),
            acknowledged_seek_revision: Some(9),
            skin_layers,
        }
    }

    #[test]
    fn acceptance_requires_a_newer_exactly_acknowledged_frame() {
        assert_eq!(classify_observation(EXPECTED, &[], observed(&[])), Ok(()));

        assert_eq!(
            classify_observation(
                EXPECTED,
                &[],
                ObservedCaptureState {
                    frame_revision: EXPECTED.baseline_frame_revision,
                    ..observed(&[])
                },
            ),
            Err(CapturePendingReason::FrameRevisionNotAdvanced {
                baseline: 4,
                observed: 4,
            })
        );
    }

    #[test]
    fn acceptance_rejects_missing_or_mismatched_command_acknowledgements() {
        assert_eq!(
            classify_observation(
                EXPECTED,
                &[],
                ObservedCaptureState {
                    acknowledged_play_revision: Some(6),
                    ..observed(&[])
                },
            ),
            Err(CapturePendingReason::PlayRevisionMismatch {
                expected: 7,
                observed: Some(6),
            })
        );
        assert_eq!(
            classify_observation(
                EXPECTED,
                &[],
                ObservedCaptureState {
                    acknowledged_seek_revision: None,
                    ..observed(&[])
                },
            ),
            Err(CapturePendingReason::SeekRevisionMismatch {
                expected: 9,
                observed: None,
            })
        );
    }

    #[test]
    fn acceptance_requires_the_complete_exact_skin_order() {
        assert_eq!(
            classify_observation(EXPECTED, &["alternate"], observed(&[])),
            Err(CapturePendingReason::SkinLayersMismatch {
                expected: vec![Box::<str>::from("alternate")],
                observed: Vec::new(),
            })
        );
        assert_eq!(
            classify_observation(
                EXPECTED,
                &["alternate", "accessory"],
                observed(&["accessory", "alternate"]),
            ),
            Err(CapturePendingReason::SkinLayersMismatch {
                expected: vec![Box::<str>::from("alternate"), Box::<str>::from("accessory"),],
                observed: vec![Box::<str>::from("accessory"), Box::<str>::from("alternate"),],
            })
        );
    }

    #[test]
    fn fixed_schedule_is_literal_and_bounded() {
        assert_eq!(NATIVE_SAMPLE_SCHEDULE.len(), NATIVE_SAMPLE_COUNT);
        assert_eq!(NativeSample::SwayStart.time(), Duration::ZERO);
        assert_eq!(NativeSample::SwayMiddle.time(), Duration::from_millis(500));
        assert_eq!(
            NativeSample::SwayAlternateSkin.time(),
            Duration::from_millis(750)
        );
        assert_eq!(NativeSample::SwayEnd.time(), Duration::from_secs(1));
        assert_eq!(
            NativeSample::SwayAlternateSkin.skin_layers(),
            &["alternate"]
        );
    }

    #[test]
    fn identity_binding_preserves_exact_roles_identities_and_distinct_poses() {
        let proposed_json = modified_smoke_json(
            "{ \"time\": 1, \"x\": 10, \"y\": 0 }",
            "{ \"time\": 1, \"x\": 20, \"y\": 0 }",
        );
        let fixture = LoadedBundlesFixture::new(&proposed_json);
        let bundles = fixture.load();

        let capture = capture_loaded_case_runtime_bundles(&bundles)
            .expect("both immutable bundles support the fixed schedule");

        assert_eq!(
            capture.current_identity().manifest_sha256(),
            bundles.current().manifest_sha256()
        );
        assert_eq!(
            capture.current_identity().content_sha256(),
            bundles.current().content_sha256()
        );
        assert_eq!(
            capture.proposed_identity().manifest_sha256(),
            bundles.proposed().manifest_sha256()
        );
        assert_eq!(
            capture.proposed_identity().content_sha256(),
            bundles.proposed().content_sha256()
        );
        assert_ne!(capture.current_identity(), capture.proposed_identity());

        let current_end = &capture.observations().current()[NATIVE_SAMPLE_COUNT - 1];
        let proposed_end = &capture.observations().proposed()[NATIVE_SAMPLE_COUNT - 1];
        assert_eq!(current_end.source(), NativeSource::Current);
        assert_eq!(proposed_end.source(), NativeSource::Proposed);
        assert_eq!(
            current_end.frame().bones()[0].local().translation()[0],
            10.0
        );
        assert_eq!(
            proposed_end.frame().bones()[0].local().translation()[0],
            20.0
        );
    }

    #[test]
    fn adapter_construction_failures_retain_the_exact_source_and_error_type() {
        let fixture = LoadedBundlesFixture::new(SMOKE_JSON);
        let bundles = fixture.load();
        let mut app = new_headless_capture_app();

        let error = insert_capture_asset(
            &mut app,
            NativeSource::Proposed,
            Arc::clone(bundles.proposed().asset()),
            Vec::new(),
        )
        .expect_err("an atlas-backed skeleton cannot be constructed without its page");

        assert!(matches!(
            error,
            IdentityBoundNativeCaptureError::AssetConstruction {
                capture_source: NativeSource::Proposed,
                source: SpinalAssetLoaderError::PageCountMismatch {
                    expected: 1,
                    actual: 0,
                },
            }
        ));
    }

    #[test]
    fn fixed_capture_failures_retain_the_exact_source_and_error_type() {
        let missing_animation = modified_smoke_json("\"sway\": {", "\"idle\": {");
        let fixture = LoadedBundlesFixture::new(&missing_animation);
        let bundles = fixture.load();

        assert!(matches!(
            capture_loaded_case_runtime_bundles(&bundles),
            Err(IdentityBoundNativeCaptureError::Capture(
                NativeCaptureError::MissingAnimation {
                    capture_source: NativeSource::Proposed,
                    animation: NATIVE_ANIMATION_NAME,
                }
            ))
        ));
    }

    #[test]
    fn identity_bound_capture_is_unconditionally_gate_ineligible() {
        let fixture = LoadedBundlesFixture::new(SMOKE_JSON);
        let bundles = fixture.load();
        let capture = capture_loaded_case_runtime_bundles(&bundles)
            .expect("the fixed fixture supports native capture");

        assert!(!capture.gate_eligible());
    }

    #[test]
    fn capture_rejects_unloaded_assets_before_spawning_or_issuing_commands() {
        let mut app = new_headless_capture_app();
        let current = Handle::<SpinalAsset>::default();
        let proposed = add_smoke_asset(&mut app);

        assert_eq!(
            capture_native_schedule(&mut app, current, proposed),
            Err(NativeCaptureError::AssetNotLoaded {
                capture_source: NativeSource::Current,
            })
        );
    }

    #[test]
    fn capture_rejects_missing_fixed_animation_on_either_source() {
        let mut app = new_headless_capture_app();
        let current = add_smoke_asset(&mut app);
        let missing = modified_smoke_json("\"sway\": {", "\"idle\": {");
        let proposed = add_asset(&mut app, &missing);

        assert_eq!(
            capture_native_schedule(&mut app, current, proposed),
            Err(NativeCaptureError::MissingAnimation {
                capture_source: NativeSource::Proposed,
                animation: NATIVE_ANIMATION_NAME,
            })
        );
    }

    #[test]
    fn capture_rejects_nonexact_fixed_animation_duration() {
        let mut app = new_headless_capture_app();
        let wrong_duration = modified_smoke_json(
            "{ \"time\": 1, \"x\": 10, \"y\": 0 }",
            "{ \"time\": 0.5, \"x\": 10, \"y\": 0 }",
        );
        let current = add_asset(&mut app, &wrong_duration);
        let proposed = add_smoke_asset(&mut app);

        assert_eq!(
            capture_native_schedule(&mut app, current, proposed),
            Err(NativeCaptureError::AnimationDurationMismatch {
                capture_source: NativeSource::Current,
                animation: NATIVE_ANIMATION_NAME,
                expected: Duration::from_secs(1),
                actual: Duration::from_millis(500),
            })
        );
    }

    #[test]
    fn capture_rejects_missing_alternate_skin() {
        let mut app = new_headless_capture_app();
        let current = add_smoke_asset(&mut app);
        let missing_skin = modified_smoke_json("\"name\": \"alternate\"", "\"name\": \"other\"");
        let proposed = add_asset(&mut app, &missing_skin);

        assert_eq!(
            capture_native_schedule(&mut app, current, proposed),
            Err(NativeCaptureError::MissingSkin {
                capture_source: NativeSource::Proposed,
                skin: ALTERNATE_SKIN_NAME,
            })
        );
    }

    #[test]
    fn two_source_headless_runtime_emits_all_bounded_schedule_observations() {
        let mut app = new_headless_capture_app();
        let current = add_smoke_asset(&mut app);
        let proposed = add_smoke_asset(&mut app);

        let observations = capture_native_schedule(&mut app, current, proposed)
            .expect("the hand-authored smoke fixture supports the closed schedule");

        assert_source_schedule(NativeSource::Current, observations.current());
        assert_source_schedule(NativeSource::Proposed, observations.proposed());
    }

    fn assert_source_schedule(
        expected_source: NativeSource,
        observations: &[NativeSemanticObservation; NATIVE_SAMPLE_COUNT],
    ) {
        for (index, (observation, sample)) in
            observations.iter().zip(NATIVE_SAMPLE_SCHEDULE).enumerate()
        {
            let generation = u64::try_from(index + 1).expect("four samples fit in u64");
            assert_eq!(observation.source(), expected_source);
            assert_eq!(observation.sample(), sample);
            assert_eq!(observation.frame_revision(), generation);
            assert_eq!(observation.acknowledged_play_revision(), generation);
            assert_eq!(
                observation.acknowledged_seek_revision(),
                generation * 2,
                "play clears the previous seek before the exact seek is issued"
            );
            assert!(
                observation
                    .frame()
                    .skin_layers()
                    .eq(sample.skin_layers().iter().copied()),
                "{} must retain the exact ordered skin selection",
                sample.id()
            );
        }
    }

    fn add_smoke_asset(app: &mut App) -> Handle<SpinalAsset> {
        add_asset(app, SMOKE_JSON)
    }

    fn modified_smoke_json(from: &str, to: &str) -> Vec<u8> {
        let source = std::str::from_utf8(SMOKE_JSON).expect("smoke JSON is UTF-8");
        assert_eq!(source.matches(from).count(), 1, "unique smoke replacement");
        source.replacen(from, to, 1).into_bytes()
    }

    fn add_asset(app: &mut App, json: &[u8]) -> Handle<SpinalAsset> {
        let skeleton = load_json(json, SMOKE_ATLAS)
            .expect("the hand-authored smoke fixture is supported")
            .into_asset();
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let asset = SpinalAsset::new(
            Arc::clone(&skeleton),
            vec![SpinalAtlasPage::new("cat.png", image)],
        )
        .expect("the in-memory page matches the hand-authored atlas");
        app.world_mut()
            .resource_mut::<Assets<SpinalAsset>>()
            .add(asset)
    }
}

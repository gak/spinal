use crate::{
    AnimationId, AtlasPageId, AtlasRegionId, AttachmentId, BoneId, ConstraintId, IkConstraintId,
    SkinId, SlotId,
};

/// The runtime impact of a non-fatal asset diagnostic.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum DiagnosticSeverity {
    /// The asset remains visually and behaviorally equivalent.
    Warning,
    /// Some visible or behavioral output is intentionally degraded.
    Degraded,
}

/// A stable machine-readable category for a non-fatal diagnostic.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// An attachment type is known but not implemented.
    UnsupportedAttachmentType,
    /// A constraint type is known but not implemented.
    UnsupportedConstraintType,
    /// A supported constraint uses an option outside the Loafstead profile.
    UnsupportedConstraintOption,
    /// A bone uses a transform or inheritance mode outside the profile.
    UnsupportedBoneTransformMode,
    /// An animation timeline type is known but not implemented.
    UnsupportedTimelineType,
    /// A slot blend mode is known but not implemented.
    UnsupportedBlendMode,
    /// Bones activated only by a skin were ignored.
    IgnoredSkinBones,
    /// Constraints activated only by a skin were ignored.
    IgnoredSkinConstraints,
    /// Optional data was malformed and ignored.
    InvalidOptionalData,
    /// The export uses a compatible but not yet conformance-tested patch.
    UntestedPatchVersion,
    /// A texture page's alpha encoding differs from the requested profile.
    AlphaEncodingMismatch,
}

/// The asset element affected by a diagnostic.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum DiagnosticScope {
    /// The diagnostic applies to the complete asset.
    Asset,
    /// The diagnostic applies while one bone affects the evaluated pose.
    Bone(BoneId),
    /// The diagnostic applies while one slot affects the draw list.
    Slot(SlotId),
    /// The diagnostic applies to one skin.
    Skin(SkinId),
    /// The diagnostic applies while one animation is active.
    Animation(AnimationId),
    /// The diagnostic applies while one attachment is visible.
    Attachment(AttachmentId),
    /// The diagnostic applies while one IK constraint is active.
    IkConstraint(IkConstraintId),
    /// The diagnostic applies while any authored constraint is active.
    Constraint(ConstraintId),
    /// The diagnostic applies while one atlas page is used.
    AtlasPage(AtlasPageId),
    /// The diagnostic applies while one atlas region is used.
    AtlasRegion(AtlasRegionId),
}

/// A non-fatal issue discovered while linking an asset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub(crate) severity: DiagnosticSeverity,
    pub(crate) code: DiagnosticCode,
    pub(crate) scope: DiagnosticScope,
    pub(crate) message: Box<str>,
}

impl Diagnostic {
    /// Returns the runtime impact.
    #[must_use]
    pub const fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    /// Returns the stable machine-readable category.
    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        self.code
    }

    /// Returns the affected asset element.
    #[must_use]
    pub const fn scope(&self) -> DiagnosticScope {
        self.scope
    }

    /// Returns the human-readable explanation.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns whether output differs from the authored asset.
    ///
    /// Only an active degraded diagnostic should trigger the Bevy adapter's
    /// red-cross indicator. Warnings never trigger it.
    #[must_use]
    pub const fn is_degraded(&self) -> bool {
        matches!(self.severity, DiagnosticSeverity::Degraded)
    }
}

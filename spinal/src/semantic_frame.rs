//! Owned, stable-name observations of solved runtime frames.

use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    AtlasPageId, AtlasRegionId, AttachmentId, BoneTransform, Diagnostic, DiagnosticCode,
    DiagnosticScope, DiagnosticSeverity, DrawItemRef, IkSolveIssue, IkTargetReach, Rgba,
    SkeletonAsset, SlotBlendMode, SolvedFrame, TransformSolveIssue, WorldTransform,
};

/// The semantic-frame JSON schema emitted by this version of Spinal.
pub const SEMANTIC_FRAME_FORMAT_VERSION: u16 = 1;

/// An owned renderer-neutral observation of one solved skeleton frame.
///
/// Every reference is represented by an authored stable name rather than an
/// asset-scoped runtime ID. Vectors retain source or evaluated draw order, so
/// independent loads of equivalent inputs produce comparable values.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SemanticFrame {
    format_version: u16,
    default_skin: Option<Box<str>>,
    skin_layers: Vec<Box<str>>,
    bones: Vec<SemanticBone>,
    slots: Vec<SemanticSlot>,
    draw_items: Vec<SemanticDraw>,
    ik_constraints: Vec<SemanticIkConstraint>,
    transform_constraints: Vec<SemanticTransformConstraint>,
    active_diagnostics: Vec<SemanticDiagnostic>,
}

impl SemanticFrame {
    /// Captures one solved frame after checking every serialized number.
    pub fn capture(frame: &SolvedFrame<'_>) -> Result<Self, SemanticFrameError> {
        let asset = frame.asset();
        let default_skin = asset.default_skin().map(|skin| skin.name().into());
        let skin_layers = frame
            .skeleton
            .skin_layers()
            .map(|skin| {
                asset
                    .skin(skin)
                    .expect("an active skin layer belongs to its immutable asset")
                    .name()
                    .into()
            })
            .collect();
        let bones = frame
            .bones()
            .map(|bone| {
                let definition = asset
                    .bone(bone.id())
                    .expect("a solved bone belongs to its immutable asset");
                Ok(SemanticBone {
                    ordinal: u32::try_from(definition.ordinal())
                        .expect("the asset loader bounds bone ordinals to u32"),
                    name: definition.name().into(),
                    local: semantic_local_transform(bone.local_transform())?,
                    world: semantic_world_transform(bone.world_transform())?,
                })
            })
            .collect::<Result<Vec<_>, SemanticFrameError>>()?;
        let slots = frame
            .slots()
            .enumerate()
            .map(|(draw_order, slot)| {
                let definition = asset
                    .slot(slot.id())
                    .expect("a solved slot belongs to its immutable asset");
                Ok(SemanticSlot {
                    draw_order: u32::try_from(draw_order)
                        .expect("the asset loader bounds slot counts to u32"),
                    name: definition.name().into(),
                    attachment: slot
                        .attachment()
                        .map(|attachment| semantic_attachment(asset, attachment)),
                    color_rgba: finite_color(slot.color(), "slot color")?,
                })
            })
            .collect::<Result<Vec<_>, SemanticFrameError>>()?;
        let draw_items = frame
            .draw_items()
            .map(|draw| semantic_draw(asset, draw))
            .collect::<Result<Vec<_>, SemanticFrameError>>()?;
        let ik_constraints = frame
            .ik_statuses()
            .map(|(constraint, status)| {
                let definition = asset
                    .ik_constraint(constraint)
                    .expect("an IK solve status belongs to its immutable asset");
                SemanticIkConstraint {
                    name: definition.name().into(),
                    active: status.is_active(),
                    preserved_underdetermined: status.preserved_underdetermined(),
                    target_reach: status.target_reach().map(semantic_ik_reach),
                    child_translation_y_zeroed: status.child_translation_y_was_zeroed(),
                    issue: status.issue().map(semantic_ik_issue),
                }
            })
            .collect();
        let transform_constraints = frame
            .transform_statuses()
            .map(|(constraint, status)| {
                let definition = asset
                    .transform_constraint(constraint)
                    .expect("a transform solve status belongs to its immutable asset");
                SemanticTransformConstraint {
                    name: definition.name().into(),
                    active: status.is_active(),
                    issue: status.issue().map(semantic_transform_issue),
                }
            })
            .collect();
        let active_diagnostics = frame
            .active_diagnostics()
            .map(|diagnostic| SemanticDiagnostic::capture(asset, diagnostic))
            .collect();

        let mut captured = Self {
            format_version: SEMANTIC_FRAME_FORMAT_VERSION,
            default_skin,
            skin_layers,
            bones,
            slots,
            draw_items,
            ik_constraints,
            transform_constraints,
            active_diagnostics,
        };
        captured.normalize_signed_zeroes();
        captured.validate()?;
        Ok(captured)
    }

    /// Parses strict semantic-frame JSON and validates its schema version and
    /// numeric invariants.
    pub fn from_json(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    /// Serializes deterministic compact JSON.
    ///
    /// The schema contains only structs and ordered vectors, and all signed
    /// floating-point zeroes are normalized, so identical semantic frames
    /// produce identical bytes.
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Returns the semantic-frame JSON schema version.
    #[must_use]
    pub const fn format_version(&self) -> u16 {
        self.format_version
    }

    /// Returns the default fallback skin, when one was exported.
    #[must_use]
    pub fn default_skin(&self) -> Option<&str> {
        self.default_skin.as_deref()
    }

    /// Returns selected attachment-only skin layers from low to high priority.
    #[must_use]
    pub fn skin_layers(&self) -> impl ExactSizeIterator<Item = &str> {
        self.skin_layers.iter().map(AsRef::as_ref)
    }

    /// Returns source-ordered solved bones.
    #[must_use]
    pub fn bones(&self) -> &[SemanticBone] {
        &self.bones
    }

    /// Returns evaluated slots in back-to-front draw order.
    #[must_use]
    pub fn slots(&self) -> &[SemanticSlot] {
        &self.slots
    }

    /// Returns supported draw items in back-to-front draw order.
    #[must_use]
    pub fn draw_items(&self) -> &[SemanticDraw] {
        &self.draw_items
    }

    /// Returns IK results in authored evaluation order.
    #[must_use]
    pub fn ik_constraints(&self) -> &[SemanticIkConstraint] {
        &self.ik_constraints
    }

    /// Returns transform-constraint results in authored evaluation order.
    #[must_use]
    pub fn transform_constraints(&self) -> &[SemanticTransformConstraint] {
        &self.transform_constraints
    }

    /// Returns retained diagnostics that affect this frame.
    #[must_use]
    pub fn active_diagnostics(&self) -> &[SemanticDiagnostic] {
        &self.active_diagnostics
    }

    fn validate(&self) -> Result<(), SemanticFrameError> {
        if self.format_version != SEMANTIC_FRAME_FORMAT_VERSION {
            return Err(SemanticFrameError::UnsupportedFormatVersion {
                expected: SEMANTIC_FRAME_FORMAT_VERSION,
                actual: self.format_version,
            });
        }
        for (ordinal, bone) in self.bones.iter().enumerate() {
            if usize::try_from(bone.ordinal) != Ok(ordinal) {
                return Err(SemanticFrameError::InvalidShape {
                    section: "bone ordinals",
                });
            }
            validate_finite(&bone.local.translation, "bone local translation")?;
            validate_finite(&[bone.local.rotation_radians], "bone local rotation")?;
            validate_finite(&bone.local.scale, "bone local scale")?;
            validate_finite(&bone.local.shear_radians, "bone local shear")?;
            validate_finite(&bone.world.translation, "bone world translation")?;
            validate_finite(&bone.world.x_axis, "bone world x axis")?;
            validate_finite(&bone.world.y_axis, "bone world y axis")?;
        }
        for (draw_order, slot) in self.slots.iter().enumerate() {
            if usize::try_from(slot.draw_order) != Ok(draw_order) {
                return Err(SemanticFrameError::InvalidShape {
                    section: "slot draw order",
                });
            }
            validate_color(&slot.color_rgba, "slot color")?;
        }
        for draw in &self.draw_items {
            for position in &draw.positions {
                validate_finite(position, "draw position")?;
            }
            if let Some(uvs) = &draw.uvs {
                for uv in uvs {
                    validate_finite(uv, "draw UV")?;
                }
                if uvs.len() != draw.positions.len() {
                    return Err(SemanticFrameError::InvalidShape {
                        section: "draw UVs",
                    });
                }
            }
            validate_color(&draw.color_rgba, "draw color")?;
            match draw.kind {
                SemanticDrawKind::Region => {
                    if draw.positions.len() != 4 || draw.triangles != [0, 1, 2, 0, 2, 3] {
                        return Err(SemanticFrameError::InvalidShape {
                            section: "region geometry",
                        });
                    }
                }
                SemanticDrawKind::Mesh => {
                    if draw.positions.len() < 3
                        || draw.triangles.is_empty()
                        || draw.triangles.len() % 3 != 0
                        || draw
                            .triangles
                            .iter()
                            .any(|index| *index as usize >= draw.positions.len())
                    {
                        return Err(SemanticFrameError::InvalidShape {
                            section: "mesh geometry",
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn normalize_signed_zeroes(&mut self) {
        for bone in &mut self.bones {
            normalize_zeroes(&mut bone.local.translation);
            normalize_zero(&mut bone.local.rotation_radians);
            normalize_zeroes(&mut bone.local.scale);
            normalize_zeroes(&mut bone.local.shear_radians);
            normalize_zeroes(&mut bone.world.translation);
            normalize_zeroes(&mut bone.world.x_axis);
            normalize_zeroes(&mut bone.world.y_axis);
        }
        for slot in &mut self.slots {
            normalize_zeroes(&mut slot.color_rgba);
        }
        for draw in &mut self.draw_items {
            for position in &mut draw.positions {
                normalize_zeroes(position);
            }
            if let Some(uvs) = &mut draw.uvs {
                for uv in uvs {
                    normalize_zeroes(uv);
                }
            }
            normalize_zeroes(&mut draw.color_rgba);
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticFrameWire {
    format_version: u16,
    default_skin: Option<Box<str>>,
    skin_layers: Vec<Box<str>>,
    bones: Vec<SemanticBone>,
    slots: Vec<SemanticSlot>,
    draw_items: Vec<SemanticDraw>,
    ik_constraints: Vec<SemanticIkConstraint>,
    transform_constraints: Vec<SemanticTransformConstraint>,
    active_diagnostics: Vec<SemanticDiagnostic>,
}

impl<'de> Deserialize<'de> for SemanticFrame {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SemanticFrameWire::deserialize(deserializer)?;
        let mut frame = Self {
            format_version: wire.format_version,
            default_skin: wire.default_skin,
            skin_layers: wire.skin_layers,
            bones: wire.bones,
            slots: wire.slots,
            draw_items: wire.draw_items,
            ik_constraints: wire.ik_constraints,
            transform_constraints: wire.transform_constraints,
            active_diagnostics: wire.active_diagnostics,
        };
        frame.normalize_signed_zeroes();
        frame.validate().map_err(de::Error::custom)?;
        Ok(frame)
    }
}

/// One solved bone identified by its authored name.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticBone {
    ordinal: u32,
    name: Box<str>,
    local: SemanticLocalTransform,
    world: SemanticWorldTransform,
}

impl SemanticBone {
    /// Returns the zero-based source-order position of this bone.
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    /// Returns the authored bone name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the final local transform after constraints.
    #[must_use]
    pub const fn local(&self) -> &SemanticLocalTransform {
        &self.local
    }

    /// Returns the final skeleton-space transform.
    #[must_use]
    pub const fn world(&self) -> &SemanticWorldTransform {
        &self.world
    }
}

/// A finite bone-local transform using radians for angular channels.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticLocalTransform {
    translation: [f32; 2],
    rotation_radians: f32,
    scale: [f32; 2],
    shear_radians: [f32; 2],
}

impl SemanticLocalTransform {
    /// Returns local X and Y translation.
    #[must_use]
    pub const fn translation(self) -> [f32; 2] {
        self.translation
    }

    /// Returns local rotation in radians.
    #[must_use]
    pub const fn rotation_radians(self) -> f32 {
        self.rotation_radians
    }

    /// Returns local X and Y scale.
    #[must_use]
    pub const fn scale(self) -> [f32; 2] {
        self.scale
    }

    /// Returns local X and Y shear in radians.
    #[must_use]
    pub const fn shear_radians(self) -> [f32; 2] {
        self.shear_radians
    }
}

/// A finite affine transform from bone-local coordinates to skeleton space.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticWorldTransform {
    translation: [f32; 2],
    x_axis: [f32; 2],
    y_axis: [f32; 2],
}

impl SemanticWorldTransform {
    /// Returns the skeleton-space origin.
    #[must_use]
    pub const fn translation(self) -> [f32; 2] {
        self.translation
    }

    /// Returns the transformed local X axis.
    #[must_use]
    pub const fn x_axis(self) -> [f32; 2] {
        self.x_axis
    }

    /// Returns the transformed local Y axis.
    #[must_use]
    pub const fn y_axis(self) -> [f32; 2] {
        self.y_axis
    }
}

/// One evaluated slot in current draw order.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticSlot {
    draw_order: u32,
    name: Box<str>,
    attachment: Option<SemanticAttachment>,
    color_rgba: [f32; 4],
}

impl SemanticSlot {
    /// Returns the zero-based evaluated back-to-front draw-order position.
    #[must_use]
    pub const fn draw_order(&self) -> u32 {
        self.draw_order
    }

    /// Returns the authored slot name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the selected concrete attachment, when present.
    #[must_use]
    pub const fn attachment(&self) -> Option<&SemanticAttachment> {
        self.attachment.as_ref()
    }

    /// Returns normalized light-color channels in RGBA order.
    #[must_use]
    pub const fn color_rgba(&self) -> [f32; 4] {
        self.color_rgba
    }
}

/// A concrete attachment qualified by stable authored names.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticAttachment {
    skin: Box<str>,
    slot: Box<str>,
    placeholder: Box<str>,
    name: Box<str>,
}

impl SemanticAttachment {
    /// Returns the containing skin name.
    #[must_use]
    pub fn skin(&self) -> &str {
        &self.skin
    }

    /// Returns the containing slot name.
    #[must_use]
    pub fn slot(&self) -> &str {
        &self.slot
    }

    /// Returns the skin-local attachment placeholder.
    #[must_use]
    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    /// Returns the concrete attachment name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A texture-atlas region qualified by its stable page and region names.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticAtlasRegion {
    page: Box<str>,
    region: Box<str>,
    sequence_index: Option<u32>,
}

impl SemanticAtlasRegion {
    /// Returns the atlas page image name.
    #[must_use]
    pub fn page(&self) -> &str {
        &self.page
    }

    /// Returns the atlas region name.
    #[must_use]
    pub fn region(&self) -> &str {
        &self.region
    }

    /// Returns the optional authored sequence index.
    #[must_use]
    pub const fn sequence_index(&self) -> Option<u32> {
        self.sequence_index
    }
}

/// The supported attachment geometry represented by a semantic draw item.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SemanticDrawKind {
    /// A rigid textured quadrilateral.
    Region,
    /// An indexed textured mesh.
    Mesh,
}

/// A stable semantic slot blend mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SemanticBlendMode {
    /// Standard source-over blending.
    Normal,
    /// Additive blending.
    Additive,
    /// Multiplicative blending.
    Multiply,
    /// Screen blending.
    Screen,
    /// A blend token this version does not recognize.
    Unknown,
}

/// One renderer-neutral draw item in back-to-front order.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticDraw {
    kind: SemanticDrawKind,
    slot: Box<str>,
    attachment: SemanticAttachment,
    atlas_region: SemanticAtlasRegion,
    blend_mode: SemanticBlendMode,
    positions: Vec<[f32; 2]>,
    uvs: Option<Vec<[f32; 2]>>,
    triangles: Vec<u32>,
    color_rgba: [f32; 4],
}

impl SemanticDraw {
    /// Returns the represented attachment geometry kind.
    #[must_use]
    pub const fn kind(&self) -> SemanticDrawKind {
        self.kind
    }

    /// Returns the authored slot name.
    #[must_use]
    pub fn slot(&self) -> &str {
        &self.slot
    }

    /// Returns the qualified concrete attachment name.
    #[must_use]
    pub const fn attachment(&self) -> &SemanticAttachment {
        &self.attachment
    }

    /// Returns the qualified atlas region name.
    #[must_use]
    pub const fn atlas_region(&self) -> &SemanticAtlasRegion {
        &self.atlas_region
    }

    /// Returns the stable slot blend mode.
    #[must_use]
    pub const fn blend_mode(&self) -> SemanticBlendMode {
        self.blend_mode
    }

    /// Returns solved skeleton-space vertex positions.
    #[must_use]
    pub fn positions(&self) -> &[[f32; 2]] {
        &self.positions
    }

    /// Returns normalized atlas-page UVs, when they can be derived safely.
    #[must_use]
    pub fn uvs(&self) -> Option<&[[f32; 2]]> {
        self.uvs.as_deref()
    }

    /// Returns triangle vertex indices in draw order.
    #[must_use]
    pub fn triangles(&self) -> &[u32] {
        &self.triangles
    }

    /// Returns the normalized final modulation color in RGBA order.
    #[must_use]
    pub const fn color_rgba(&self) -> [f32; 4] {
        self.color_rgba
    }
}

/// A stable classification of a two-bone IK target's reach.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SemanticIkTargetReach {
    /// The target is within the chain's geometric reach.
    Reachable,
    /// The target lies beyond the chain's geometric reach.
    BeyondReach,
}

/// A stable IK solve issue classification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SemanticIkSolveIssue {
    /// The required transform was singular or geometrically underdetermined.
    SingularOrUnderdetermined,
}

/// One IK solve result identified by the authored constraint name.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticIkConstraint {
    name: Box<str>,
    active: bool,
    preserved_underdetermined: bool,
    target_reach: Option<SemanticIkTargetReach>,
    child_translation_y_zeroed: bool,
    issue: Option<SemanticIkSolveIssue>,
}

impl SemanticIkConstraint {
    /// Returns the authored IK constraint name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns whether the constraint had nonzero influence.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Returns whether coincident geometry deliberately retained FK rotation.
    #[must_use]
    pub const fn preserved_underdetermined(&self) -> bool {
        self.preserved_underdetermined
    }

    /// Returns `reachable` or `beyond_reach` for a classified two-bone target.
    #[must_use]
    pub const fn target_reach(&self) -> Option<SemanticIkTargetReach> {
        self.target_reach
    }

    /// Returns whether two-bone IK reset the child's local Y translation.
    #[must_use]
    pub const fn child_translation_y_was_zeroed(&self) -> bool {
        self.child_translation_y_zeroed
    }

    /// Returns the stable runtime issue code, when solving was unsafe.
    #[must_use]
    pub const fn issue(&self) -> Option<SemanticIkSolveIssue> {
        self.issue
    }
}

/// A stable transform-constraint solve issue classification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SemanticTransformSolveIssue {
    /// The required transform was singular or geometrically underdetermined.
    SingularOrUnderdetermined,
}

/// One transform-constraint result identified by its authored name.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticTransformConstraint {
    name: Box<str>,
    active: bool,
    issue: Option<SemanticTransformSolveIssue>,
}

impl SemanticTransformConstraint {
    /// Returns the authored transform-constraint name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns whether the supported rotation channel had nonzero influence.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Returns the stable runtime issue code, when solving was unsafe.
    #[must_use]
    pub const fn issue(&self) -> Option<SemanticTransformSolveIssue> {
        self.issue
    }
}

/// A stable-name scope for one retained diagnostic.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SemanticDiagnosticScope {
    /// The complete asset.
    Asset,
    /// One authored bone name.
    Bone(Box<str>),
    /// One authored slot name.
    Slot(Box<str>),
    /// One authored skin name.
    Skin(Box<str>),
    /// One authored animation name.
    Animation(Box<str>),
    /// One authored event-definition name.
    Event(Box<str>),
    /// One qualified concrete attachment.
    Attachment(SemanticAttachment),
    /// One authored IK constraint name.
    IkConstraint(Box<str>),
    /// One authored constraint name.
    Constraint(Box<str>),
    /// One atlas page image name.
    AtlasPage(Box<str>),
    /// One qualified atlas region.
    AtlasRegion(SemanticAtlasRegion),
}

/// A stable diagnostic severity classification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SemanticDiagnosticSeverity {
    /// A supported-profile warning that does not change runtime behavior.
    Warning,
    /// A condition that causes deliberate degraded runtime behavior.
    Degraded,
}

impl From<DiagnosticSeverity> for SemanticDiagnosticSeverity {
    fn from(value: DiagnosticSeverity) -> Self {
        semantic_diagnostic_severity(value)
    }
}

/// A stable diagnostic classification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SemanticDiagnosticCode {
    /// An attachment type is outside the supported runtime profile.
    UnsupportedAttachmentType,
    /// A constraint type is outside the supported runtime profile.
    UnsupportedConstraintType,
    /// A constraint option is outside the supported runtime profile.
    UnsupportedConstraintOption,
    /// A bone transform mode is outside the supported runtime profile.
    UnsupportedBoneTransformMode,
    /// An animation timeline type is outside the supported runtime profile.
    UnsupportedTimelineType,
    /// A slot blend mode is outside the supported runtime profile.
    UnsupportedBlendMode,
    /// Two-colour slot tinting is outside the supported runtime profile.
    UnsupportedTwoColourTint,
    /// Skin bone declarations were intentionally ignored.
    IgnoredSkinBones,
    /// Skin constraint declarations were intentionally ignored.
    IgnoredSkinConstraints,
    /// An unknown source field was retained as a warning.
    UnknownField,
    /// The source uses an untested Spine patch version.
    UntestedPatchVersion,
    /// Texture alpha encoding disagrees with the declared runtime mode.
    AlphaEncodingMismatch,
    /// An atlas setting is outside the supported runtime profile.
    UnsupportedAtlasSetting,
    /// An atlas rotation is outside the supported runtime profile.
    UnsupportedAtlasRotation,
    /// The diagnostic budget was exceeded.
    DiagnosticsTruncated,
}

impl From<DiagnosticCode> for SemanticDiagnosticCode {
    fn from(value: DiagnosticCode) -> Self {
        semantic_diagnostic_code(value)
    }
}

/// An owned stable-name projection of one retained diagnostic.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticDiagnostic {
    severity: SemanticDiagnosticSeverity,
    code: SemanticDiagnosticCode,
    scope: SemanticDiagnosticScope,
    message: Box<str>,
}

impl SemanticDiagnostic {
    /// Captures one retained diagnostic with stable authored names in place of
    /// asset-scoped runtime identifiers.
    ///
    /// # Panics
    ///
    /// Panics when the diagnostic has a scoped identifier that does not belong
    /// to `asset`.
    #[must_use]
    pub fn capture(asset: &SkeletonAsset, diagnostic: &Diagnostic) -> Self {
        Self {
            severity: diagnostic.severity().into(),
            code: diagnostic.code().into(),
            scope: semantic_diagnostic_scope(asset, diagnostic.scope()),
            message: diagnostic.message().into(),
        }
    }

    /// Returns the stable severity classification.
    #[must_use]
    pub const fn severity(&self) -> SemanticDiagnosticSeverity {
        self.severity
    }

    /// Returns the stable diagnostic classification.
    #[must_use]
    pub const fn code(&self) -> SemanticDiagnosticCode {
        self.code
    }

    /// Returns the stable-name diagnostic scope.
    #[must_use]
    pub const fn scope(&self) -> &SemanticDiagnosticScope {
        &self.scope
    }

    /// Returns the retained human-readable detail.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// A solved frame could not be represented as valid semantic evidence.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum SemanticFrameError {
    /// A numeric channel contains NaN or infinity.
    #[error("semantic frame contains a non-finite value in {section}")]
    NonFinite {
        /// The rejected semantic section.
        section: &'static str,
    },
    /// A variable-length geometry table violates its kind's invariant.
    #[error("semantic frame contains an invalid numeric shape in {section}")]
    InvalidShape {
        /// The rejected semantic section.
        section: &'static str,
    },
    /// A finite numeric channel lies outside its semantic domain.
    #[error("semantic frame contains an invalid numeric value in {section}")]
    InvalidValue {
        /// The rejected semantic section.
        section: &'static str,
    },
    /// JSON declares a schema version this build does not understand.
    #[error("unsupported semantic-frame format version {actual}; expected {expected}")]
    UnsupportedFormatVersion {
        /// The one version accepted by this build.
        expected: u16,
        /// The version declared by the input.
        actual: u16,
    },
}

impl SemanticFrameError {
    /// Returns the rejected semantic section.
    #[must_use]
    pub const fn section(self) -> Option<&'static str> {
        match self {
            Self::NonFinite { section }
            | Self::InvalidShape { section }
            | Self::InvalidValue { section } => Some(section),
            Self::UnsupportedFormatVersion { .. } => None,
        }
    }
}

fn semantic_local_transform(
    transform: BoneTransform,
) -> Result<SemanticLocalTransform, SemanticFrameError> {
    let shear = transform.shear();
    let value = SemanticLocalTransform {
        translation: finite_vec2(transform.translation(), "bone local translation")?,
        rotation_radians: finite_scalar(transform.rotation().as_radians(), "bone local rotation")?,
        scale: finite_vec2(transform.scale(), "bone local scale")?,
        shear_radians: [
            finite_scalar(shear.x().as_radians(), "bone local shear")?,
            finite_scalar(shear.y().as_radians(), "bone local shear")?,
        ],
    };
    Ok(value)
}

fn semantic_world_transform(
    transform: WorldTransform,
) -> Result<SemanticWorldTransform, SemanticFrameError> {
    Ok(SemanticWorldTransform {
        translation: finite_vec2(transform.translation(), "bone world translation")?,
        x_axis: finite_vec2(transform.x_axis(), "bone world x axis")?,
        y_axis: finite_vec2(transform.y_axis(), "bone world y axis")?,
    })
}

fn semantic_attachment(asset: &SkeletonAsset, id: AttachmentId) -> SemanticAttachment {
    let attachment = asset
        .attachment(id)
        .expect("a runtime attachment belongs to its immutable asset");
    let skin = asset
        .skin(attachment.skin())
        .expect("a linked attachment skin belongs to its immutable asset");
    let slot = asset
        .slot(attachment.slot())
        .expect("a linked attachment slot belongs to its immutable asset");
    SemanticAttachment {
        skin: skin.name().into(),
        slot: slot.name().into(),
        placeholder: attachment.placeholder_name().into(),
        name: attachment.name().into(),
    }
}

fn semantic_atlas_region(asset: &SkeletonAsset, id: AtlasRegionId) -> SemanticAtlasRegion {
    let region = asset
        .atlas_region(id)
        .expect("a draw atlas region belongs to its immutable asset");
    let page = asset
        .atlas_page(region.page())
        .expect("a linked atlas page belongs to its immutable asset");
    SemanticAtlasRegion {
        page: page.name().into(),
        region: region.name().into(),
        sequence_index: region.index(),
    }
}

fn semantic_draw(
    asset: &SkeletonAsset,
    draw: DrawItemRef<'_>,
) -> Result<SemanticDraw, SemanticFrameError> {
    match draw {
        DrawItemRef::Region(region) => {
            let positions = region
                .positions()
                .into_iter()
                .map(|position| finite_vec2(position, "region position"))
                .collect::<Result<Vec<_>, _>>()?;
            let uvs = region
                .uvs()
                .map(|uvs| {
                    uvs.into_iter()
                        .map(|uv| finite_vec2(uv, "region UV"))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?;
            semantic_draw_record(
                asset,
                SemanticDrawKind::Region,
                region.slot(),
                region.attachment(),
                region.atlas_page(),
                region.atlas_region(),
                positions,
                uvs,
                vec![0, 1, 2, 0, 2, 3],
                region.color(),
            )
        }
        DrawItemRef::Mesh(mesh) => {
            let positions = mesh
                .positions()
                .iter()
                .copied()
                .map(|position| finite_vec2(position, "mesh position"))
                .collect::<Result<Vec<_>, _>>()?;
            let uvs = mesh
                .uvs()
                .map(|uvs| {
                    uvs.map(|uv| finite_vec2(uv, "mesh UV"))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?;
            semantic_draw_record(
                asset,
                SemanticDrawKind::Mesh,
                mesh.slot(),
                mesh.attachment(),
                mesh.atlas_page(),
                mesh.atlas_region(),
                positions,
                uvs,
                mesh.triangles().to_vec(),
                mesh.color(),
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn semantic_draw_record(
    asset: &SkeletonAsset,
    kind: SemanticDrawKind,
    slot_id: crate::SlotId,
    attachment_id: AttachmentId,
    page_id: AtlasPageId,
    region_id: AtlasRegionId,
    positions: Vec<[f32; 2]>,
    uvs: Option<Vec<[f32; 2]>>,
    triangles: Vec<u32>,
    color: Rgba,
) -> Result<SemanticDraw, SemanticFrameError> {
    let slot = asset
        .slot(slot_id)
        .expect("a draw slot belongs to its immutable asset");
    let region = semantic_atlas_region(asset, region_id);
    debug_assert_eq!(
        region.page(),
        asset
            .atlas_page(page_id)
            .expect("a draw page belongs to its immutable asset")
            .name()
    );
    Ok(SemanticDraw {
        kind,
        slot: slot.name().into(),
        attachment: semantic_attachment(asset, attachment_id),
        atlas_region: region,
        blend_mode: semantic_blend_mode(slot.blend_mode()),
        positions,
        uvs,
        triangles,
        color_rgba: finite_color(color, "draw color")?,
    })
}

fn semantic_blend_mode(mode: SlotBlendMode) -> SemanticBlendMode {
    match mode {
        SlotBlendMode::Normal => SemanticBlendMode::Normal,
        SlotBlendMode::Additive => SemanticBlendMode::Additive,
        SlotBlendMode::Multiply => SemanticBlendMode::Multiply,
        SlotBlendMode::Screen => SemanticBlendMode::Screen,
        SlotBlendMode::Unknown => SemanticBlendMode::Unknown,
    }
}

fn semantic_ik_reach(reach: IkTargetReach) -> SemanticIkTargetReach {
    match reach {
        IkTargetReach::Reachable => SemanticIkTargetReach::Reachable,
        IkTargetReach::BeyondReach => SemanticIkTargetReach::BeyondReach,
    }
}

fn semantic_ik_issue(issue: IkSolveIssue) -> SemanticIkSolveIssue {
    match issue {
        IkSolveIssue::SingularOrUnderdetermined => SemanticIkSolveIssue::SingularOrUnderdetermined,
    }
}

fn semantic_transform_issue(issue: TransformSolveIssue) -> SemanticTransformSolveIssue {
    match issue {
        TransformSolveIssue::SingularOrUnderdetermined => {
            SemanticTransformSolveIssue::SingularOrUnderdetermined
        }
    }
}

fn semantic_diagnostic_severity(severity: DiagnosticSeverity) -> SemanticDiagnosticSeverity {
    match severity {
        DiagnosticSeverity::Warning => SemanticDiagnosticSeverity::Warning,
        DiagnosticSeverity::Degraded => SemanticDiagnosticSeverity::Degraded,
    }
}

fn semantic_diagnostic_code(code: DiagnosticCode) -> SemanticDiagnosticCode {
    match code {
        DiagnosticCode::UnsupportedAttachmentType => {
            SemanticDiagnosticCode::UnsupportedAttachmentType
        }
        DiagnosticCode::UnsupportedConstraintType => {
            SemanticDiagnosticCode::UnsupportedConstraintType
        }
        DiagnosticCode::UnsupportedConstraintOption => {
            SemanticDiagnosticCode::UnsupportedConstraintOption
        }
        DiagnosticCode::UnsupportedBoneTransformMode => {
            SemanticDiagnosticCode::UnsupportedBoneTransformMode
        }
        DiagnosticCode::UnsupportedTimelineType => SemanticDiagnosticCode::UnsupportedTimelineType,
        DiagnosticCode::UnsupportedBlendMode => SemanticDiagnosticCode::UnsupportedBlendMode,
        DiagnosticCode::UnsupportedTwoColourTint => {
            SemanticDiagnosticCode::UnsupportedTwoColourTint
        }
        DiagnosticCode::IgnoredSkinBones => SemanticDiagnosticCode::IgnoredSkinBones,
        DiagnosticCode::IgnoredSkinConstraints => SemanticDiagnosticCode::IgnoredSkinConstraints,
        DiagnosticCode::UnknownField => SemanticDiagnosticCode::UnknownField,
        DiagnosticCode::UntestedPatchVersion => SemanticDiagnosticCode::UntestedPatchVersion,
        DiagnosticCode::AlphaEncodingMismatch => SemanticDiagnosticCode::AlphaEncodingMismatch,
        DiagnosticCode::UnsupportedAtlasSetting => SemanticDiagnosticCode::UnsupportedAtlasSetting,
        DiagnosticCode::UnsupportedAtlasRotation => {
            SemanticDiagnosticCode::UnsupportedAtlasRotation
        }
        DiagnosticCode::DiagnosticsTruncated => SemanticDiagnosticCode::DiagnosticsTruncated,
    }
}

fn semantic_diagnostic_scope(
    asset: &SkeletonAsset,
    scope: DiagnosticScope,
) -> SemanticDiagnosticScope {
    match scope {
        DiagnosticScope::Asset => SemanticDiagnosticScope::Asset,
        DiagnosticScope::Bone(id) => SemanticDiagnosticScope::Bone(
            asset
                .bone(id)
                .expect("a diagnostic bone belongs to its immutable asset")
                .name()
                .into(),
        ),
        DiagnosticScope::Slot(id) => SemanticDiagnosticScope::Slot(
            asset
                .slot(id)
                .expect("a diagnostic slot belongs to its immutable asset")
                .name()
                .into(),
        ),
        DiagnosticScope::Skin(id) => SemanticDiagnosticScope::Skin(
            asset
                .skin(id)
                .expect("a diagnostic skin belongs to its immutable asset")
                .name()
                .into(),
        ),
        DiagnosticScope::Animation(id) => SemanticDiagnosticScope::Animation(
            asset
                .animation(id)
                .expect("a diagnostic animation belongs to its immutable asset")
                .name()
                .into(),
        ),
        DiagnosticScope::Event(id) => SemanticDiagnosticScope::Event(
            asset
                .event_definition(id)
                .expect("a diagnostic event belongs to its immutable asset")
                .name()
                .into(),
        ),
        DiagnosticScope::Attachment(id) => {
            SemanticDiagnosticScope::Attachment(semantic_attachment(asset, id))
        }
        DiagnosticScope::IkConstraint(id) => SemanticDiagnosticScope::IkConstraint(
            asset
                .ik_constraint(id)
                .expect("a diagnostic IK constraint belongs to its immutable asset")
                .name()
                .into(),
        ),
        DiagnosticScope::Constraint(id) => SemanticDiagnosticScope::Constraint(
            asset
                .constraint(id)
                .expect("a diagnostic constraint belongs to its immutable asset")
                .name()
                .into(),
        ),
        DiagnosticScope::AtlasPage(id) => SemanticDiagnosticScope::AtlasPage(
            asset
                .atlas_page(id)
                .expect("a diagnostic atlas page belongs to its immutable asset")
                .name()
                .into(),
        ),
        DiagnosticScope::AtlasRegion(id) => {
            SemanticDiagnosticScope::AtlasRegion(semantic_atlas_region(asset, id))
        }
    }
}

fn finite_vec2(
    value: crate::glam::Vec2,
    section: &'static str,
) -> Result<[f32; 2], SemanticFrameError> {
    if value.is_finite() {
        Ok(value.to_array())
    } else {
        Err(SemanticFrameError::NonFinite { section })
    }
}

fn finite_color(value: Rgba, section: &'static str) -> Result<[f32; 4], SemanticFrameError> {
    let channels = value.to_array();
    if channels.into_iter().all(f32::is_finite) {
        Ok(channels)
    } else {
        Err(SemanticFrameError::NonFinite { section })
    }
}

fn finite_scalar(value: f32, section: &'static str) -> Result<f32, SemanticFrameError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(SemanticFrameError::NonFinite { section })
    }
}

fn validate_finite(values: &[f32], section: &'static str) -> Result<(), SemanticFrameError> {
    if values.iter().copied().all(f32::is_finite) {
        Ok(())
    } else {
        Err(SemanticFrameError::NonFinite { section })
    }
}

fn validate_color(values: &[f32; 4], section: &'static str) -> Result<(), SemanticFrameError> {
    validate_finite(values, section)?;
    if values.iter().all(|channel| (0.0..=1.0).contains(channel)) {
        Ok(())
    } else {
        Err(SemanticFrameError::InvalidValue { section })
    }
}

fn normalize_zero(value: &mut f32) {
    if *value == 0.0 {
        *value = 0.0;
    }
}

fn normalize_zeroes(values: &mut [f32]) {
    for value in values {
        normalize_zero(value);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};

    use super::*;
    use crate::load_json;

    const DIAGNOSTIC_ATLAS: &str = "\
page.png
\tsize: 16, 16
region
\tbounds: 0, 0, 8, 8
";
    const DIAGNOSTIC_JSON: &str = r#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[
        {"name":"root"},
        {"name":"constrained","parent":"root"},
        {"name":"target","parent":"root"}
      ],
      "slots":[{"name":"slot","bone":"root","attachment":"region"}],
      "skins":[{
        "name":"default",
        "attachments":{"slot":{"region":{"width":8,"height":8}}}
      }],
      "constraints":[{
        "name":"ik","type":"ik","bones":["constrained"],"target":"target"
      }],
      "events":{"ping":{}},
      "animations":{"idle":{}}
    }"#;

    fn diagnostic_asset() -> Arc<SkeletonAsset> {
        load_json(DIAGNOSTIC_JSON.as_bytes(), DIAGNOSTIC_ATLAS.as_bytes())
            .expect("the diagnostic fixture should load")
            .into_asset()
    }

    fn capture_value(
        asset: &SkeletonAsset,
        severity: DiagnosticSeverity,
        code: DiagnosticCode,
        scope: DiagnosticScope,
    ) -> Value {
        let diagnostic = Diagnostic {
            severity,
            code,
            scope,
            message: "stable detail".into(),
        };
        serde_json::to_value(SemanticDiagnostic::capture(asset, &diagnostic))
            .expect("a semantic diagnostic should serialize")
    }

    #[test]
    fn capture_serializes_every_current_severity_and_code_with_stable_tokens() {
        let asset = diagnostic_asset();
        for (severity, expected) in [
            (DiagnosticSeverity::Warning, "warning"),
            (DiagnosticSeverity::Degraded, "degraded"),
        ] {
            let value = capture_value(
                &asset,
                severity,
                DiagnosticCode::UnknownField,
                DiagnosticScope::Asset,
            );
            assert_eq!(value["severity"], expected);
        }

        for (code, expected) in [
            (
                DiagnosticCode::UnsupportedAttachmentType,
                "unsupported_attachment_type",
            ),
            (
                DiagnosticCode::UnsupportedConstraintType,
                "unsupported_constraint_type",
            ),
            (
                DiagnosticCode::UnsupportedConstraintOption,
                "unsupported_constraint_option",
            ),
            (
                DiagnosticCode::UnsupportedBoneTransformMode,
                "unsupported_bone_transform_mode",
            ),
            (
                DiagnosticCode::UnsupportedTimelineType,
                "unsupported_timeline_type",
            ),
            (
                DiagnosticCode::UnsupportedBlendMode,
                "unsupported_blend_mode",
            ),
            (
                DiagnosticCode::UnsupportedTwoColourTint,
                "unsupported_two_colour_tint",
            ),
            (DiagnosticCode::IgnoredSkinBones, "ignored_skin_bones"),
            (
                DiagnosticCode::IgnoredSkinConstraints,
                "ignored_skin_constraints",
            ),
            (DiagnosticCode::UnknownField, "unknown_field"),
            (
                DiagnosticCode::UntestedPatchVersion,
                "untested_patch_version",
            ),
            (
                DiagnosticCode::AlphaEncodingMismatch,
                "alpha_encoding_mismatch",
            ),
            (
                DiagnosticCode::UnsupportedAtlasSetting,
                "unsupported_atlas_setting",
            ),
            (
                DiagnosticCode::UnsupportedAtlasRotation,
                "unsupported_atlas_rotation",
            ),
            (
                DiagnosticCode::DiagnosticsTruncated,
                "diagnostics_truncated",
            ),
        ] {
            let value = capture_value(
                &asset,
                DiagnosticSeverity::Warning,
                code,
                DiagnosticScope::Asset,
            );
            assert_eq!(value["code"], expected);
        }
    }

    #[test]
    fn capture_serializes_every_current_scope_with_stable_authored_names() {
        let asset = diagnostic_asset();
        let attachment = asset
            .attachments()
            .next()
            .expect("the fixture has an attachment");
        let atlas_region = asset
            .atlas_regions()
            .next()
            .expect("the fixture has an atlas region");
        let scopes = [
            (DiagnosticScope::Asset, json!({"kind":"asset"})),
            (
                DiagnosticScope::Bone(asset.bone_id("root").expect("root exists")),
                json!({"kind":"bone","value":"root"}),
            ),
            (
                DiagnosticScope::Slot(asset.slot_id("slot").expect("slot exists")),
                json!({"kind":"slot","value":"slot"}),
            ),
            (
                DiagnosticScope::Skin(asset.skin_id("default").expect("default skin exists")),
                json!({"kind":"skin","value":"default"}),
            ),
            (
                DiagnosticScope::Animation(asset.animation_id("idle").expect("idle exists")),
                json!({"kind":"animation","value":"idle"}),
            ),
            (
                DiagnosticScope::Event(asset.event_id("ping").expect("ping exists")),
                json!({"kind":"event","value":"ping"}),
            ),
            (
                DiagnosticScope::Attachment(attachment.id()),
                json!({
                    "kind":"attachment",
                    "value":{
                        "skin":"default",
                        "slot":"slot",
                        "placeholder":"region",
                        "name":"region"
                    }
                }),
            ),
            (
                DiagnosticScope::IkConstraint(
                    asset.ik_constraint_id("ik").expect("IK constraint exists"),
                ),
                json!({"kind":"ik_constraint","value":"ik"}),
            ),
            (
                DiagnosticScope::Constraint(asset.constraint_id("ik").expect("constraint exists")),
                json!({"kind":"constraint","value":"ik"}),
            ),
            (
                DiagnosticScope::AtlasPage(
                    asset.atlas_page_id("page.png").expect("atlas page exists"),
                ),
                json!({"kind":"atlas_page","value":"page.png"}),
            ),
            (
                DiagnosticScope::AtlasRegion(atlas_region.id()),
                json!({
                    "kind":"atlas_region",
                    "value":{
                        "page":"page.png",
                        "region":"region",
                        "sequence_index":null
                    }
                }),
            ),
        ];

        for (scope, expected) in scopes {
            let value = capture_value(
                &asset,
                DiagnosticSeverity::Degraded,
                DiagnosticCode::UnsupportedAttachmentType,
                scope,
            );
            assert_eq!(value["scope"], expected);
            assert_eq!(value["message"], "stable detail");
        }
    }
}

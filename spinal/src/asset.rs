use std::{collections::HashMap, ops::Range, time::Duration};

use crate::{
    AlphaEncoding, AnimationId, AtlasPageId, AtlasRegionId, AtlasRotation, AttachmentId, BoneId,
    BoneTransform, ConstraintId, Diagnostic, EventId, IdError, IkConstraintId, Mix, PixelRect,
    PixelSize, Rgba8, SkinId, SlotId, TextureFilter, TextureFormat, Trim, WrapMode,
    animation::{AnimationData, EventDefinitionData},
    id::AssetKey,
};

#[derive(Debug)]
pub(crate) struct BoneData {
    pub(crate) name: Box<str>,
    pub(crate) parent: Option<u32>,
    pub(crate) length: f32,
    pub(crate) setup_transform: BoneTransform,
}

#[derive(Debug)]
pub(crate) struct SlotData {
    pub(crate) name: Box<str>,
    pub(crate) bone: u32,
    pub(crate) setup_attachment: Option<u32>,
    pub(crate) colour: Rgba8,
    pub(crate) blend_mode: SlotBlendMode,
    pub(crate) blend_token: Box<str>,
}

#[derive(Debug)]
pub(crate) struct SkinData {
    pub(crate) name: Box<str>,
    pub(crate) attachments: Range<u32>,
}

#[derive(Debug)]
pub(crate) struct AttachmentData {
    pub(crate) placeholder_name: Box<str>,
    pub(crate) name: Box<str>,
    pub(crate) atlas_path: Option<Box<str>>,
    pub(crate) skin: u32,
    pub(crate) slot: u32,
    pub(crate) kind: AttachmentDataKind,
}

#[derive(Debug)]
pub(crate) enum AttachmentDataKind {
    Region(RegionAttachmentData),
    BoundingBox,
    Point,
    Unsupported { source_type: Box<str> },
}

#[derive(Debug)]
pub(crate) struct RegionAttachmentData {
    pub(crate) transform: BoneTransform,
    pub(crate) size: PixelSize,
    pub(crate) colour: Rgba8,
    pub(crate) atlas_region: u32,
}

#[derive(Debug)]
pub(crate) struct AtlasExtension {
    pub(crate) key: Box<str>,
    pub(crate) value: Box<str>,
}

#[derive(Debug)]
pub(crate) struct AtlasPageData {
    pub(crate) name: Box<str>,
    pub(crate) size: PixelSize,
    pub(crate) format: TextureFormat,
    pub(crate) format_token: Box<str>,
    pub(crate) min_filter: TextureFilter,
    pub(crate) min_filter_token: Box<str>,
    pub(crate) mag_filter: TextureFilter,
    pub(crate) mag_filter_token: Box<str>,
    pub(crate) wrap: WrapMode,
    pub(crate) alpha_encoding: AlphaEncoding,
    pub(crate) scale: f32,
    pub(crate) regions: Range<u32>,
    pub(crate) extensions: Box<[AtlasExtension]>,
}

#[derive(Debug)]
pub(crate) struct AtlasRegionData {
    pub(crate) name: Box<str>,
    pub(crate) page: u32,
    pub(crate) index: Option<u32>,
    pub(crate) bounds: PixelRect,
    pub(crate) trim: Trim,
    pub(crate) rotation: AtlasRotation,
    pub(crate) split: Option<[i32; 4]>,
    pub(crate) pad: Option<[i32; 4]>,
    pub(crate) extensions: Box<[AtlasExtension]>,
}

#[derive(Debug)]
pub(crate) struct IkConstraintData {
    pub(crate) constraint: u32,
    pub(crate) name: Box<str>,
    pub(crate) order: u32,
    pub(crate) bones: Box<[u32]>,
    pub(crate) target: u32,
    pub(crate) mix: Mix,
    pub(crate) bend_direction: BendDirection,
    pub(crate) softness: f32,
    pub(crate) compress: bool,
    pub(crate) stretch: bool,
    pub(crate) uniform: bool,
}

#[derive(Debug)]
pub(crate) struct ConstraintData {
    pub(crate) name: Box<str>,
    pub(crate) source_type: Box<str>,
    pub(crate) order: u32,
    pub(crate) ik_constraint: Option<u32>,
}

#[derive(Debug)]
pub(crate) struct AssetData {
    pub(crate) spine_version: Box<str>,
    pub(crate) bones: Box<[BoneData]>,
    pub(crate) slots: Box<[SlotData]>,
    pub(crate) skins: Box<[SkinData]>,
    pub(crate) attachments: Box<[AttachmentData]>,
    pub(crate) animations: Box<[AnimationData]>,
    pub(crate) ik_constraints: Box<[IkConstraintData]>,
    pub(crate) constraints: Box<[ConstraintData]>,
    pub(crate) atlas_pages: Box<[AtlasPageData]>,
    pub(crate) atlas_regions: Box<[AtlasRegionData]>,
    pub(crate) events: Box<[EventDefinitionData]>,
    pub(crate) diagnostics: Box<[Diagnostic]>,
}

/// The supported or retained category of an authored attachment.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum AttachmentKind {
    /// A rigid textured quad.
    Region,
    /// Non-rendered hit geometry retained as metadata.
    BoundingBox,
    /// A non-rendered authored point retained as metadata.
    Point,
    /// A known attachment whose semantics are outside the active profile.
    Unsupported,
}

/// The bend direction for a one- or two-bone IK constraint.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum BendDirection {
    /// Bend toward the mathematically positive solution.
    #[default]
    Positive,
    /// Bend toward the mathematically negative solution.
    Negative,
}

/// An authored slot blend mode.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum SlotBlendMode {
    /// Standard source-over blending.
    #[default]
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

/// Immutable, linked skeleton data shared by runtime instances.
///
/// A loaded asset is intentionally not `Clone`. Share ownership through
/// [`std::sync::Arc`] so every runtime instance retains the identity used by
/// its typed IDs.
#[derive(Debug)]
pub struct SkeletonAsset {
    key: AssetKey,
    spine_version: Box<str>,
    bones: Box<[BoneData]>,
    slots: Box<[SlotData]>,
    skins: Box<[SkinData]>,
    attachments: Box<[AttachmentData]>,
    animations: Box<[AnimationData]>,
    ik_constraints: Box<[IkConstraintData]>,
    constraints: Box<[ConstraintData]>,
    atlas_pages: Box<[AtlasPageData]>,
    atlas_regions: Box<[AtlasRegionData]>,
    events: Box<[EventDefinitionData]>,
    bone_by_name: HashMap<Box<str>, u32>,
    slot_by_name: HashMap<Box<str>, u32>,
    skin_by_name: HashMap<Box<str>, u32>,
    animation_by_name: HashMap<Box<str>, u32>,
    ik_constraint_by_name: HashMap<Box<str>, u32>,
    constraint_by_name: HashMap<Box<str>, u32>,
    event_by_name: HashMap<Box<str>, u32>,
    atlas_page_by_name: HashMap<Box<str>, u32>,
    atlas_regions_by_name: HashMap<Box<str>, Box<[u32]>>,
    attachment_by_skin_slot: HashMap<(u32, u32), HashMap<Box<str>, u32>>,
    diagnostics: Box<[Diagnostic]>,
}

impl SkeletonAsset {
    pub(crate) fn from_data(key: AssetKey, data: AssetData) -> Self {
        let bone_by_name = lookup(&data.bones, |bone| &bone.name);
        let slot_by_name = lookup(&data.slots, |slot| &slot.name);
        let skin_by_name = lookup(&data.skins, |skin| &skin.name);
        let animation_by_name = lookup(&data.animations, |animation| &animation.name);
        let ik_constraint_by_name = lookup(&data.ik_constraints, |constraint| &constraint.name);
        let constraint_by_name = lookup(&data.constraints, |constraint| &constraint.name);
        let event_by_name = lookup(&data.events, |event| &event.name);
        let atlas_page_by_name = lookup(&data.atlas_pages, |page| &page.name);
        let mut region_indexes = HashMap::<Box<str>, Vec<u32>>::new();
        for (index, region) in data.atlas_regions.iter().enumerate() {
            let index = u32::try_from(index)
                .expect("validated atlas region tables fit the asset-scoped ID representation");
            region_indexes
                .entry(region.name.clone())
                .or_default()
                .push(index);
        }
        let atlas_regions_by_name = region_indexes
            .into_iter()
            .map(|(name, indexes)| (name, indexes.into_boxed_slice()))
            .collect();
        let mut attachment_by_skin_slot = HashMap::<(u32, u32), HashMap<Box<str>, u32>>::new();
        for (index, attachment) in data.attachments.iter().enumerate() {
            let index = u32::try_from(index)
                .expect("validated attachment tables fit the asset-scoped ID representation");
            attachment_by_skin_slot
                .entry((attachment.skin, attachment.slot))
                .or_default()
                .insert(attachment.placeholder_name.clone(), index);
        }

        Self {
            key,
            spine_version: data.spine_version,
            bones: data.bones,
            slots: data.slots,
            skins: data.skins,
            attachments: data.attachments,
            animations: data.animations,
            ik_constraints: data.ik_constraints,
            constraints: data.constraints,
            atlas_pages: data.atlas_pages,
            atlas_regions: data.atlas_regions,
            events: data.events,
            bone_by_name,
            slot_by_name,
            skin_by_name,
            animation_by_name,
            ik_constraint_by_name,
            constraint_by_name,
            event_by_name,
            atlas_page_by_name,
            atlas_regions_by_name,
            attachment_by_skin_slot,
            diagnostics: data.diagnostics,
        }
    }

    /// Returns the Spine editor wire-format version recorded by the export.
    #[must_use]
    pub fn spine_version(&self) -> &str {
        &self.spine_version
    }

    /// Resolves a bone name without allocating.
    #[must_use]
    pub fn bone_id(&self, name: &str) -> Option<BoneId> {
        self.bone_by_name
            .get(name)
            .copied()
            .map(|index| BoneId::new(self.key, index))
    }

    /// Resolves a slot name without allocating.
    #[must_use]
    pub fn slot_id(&self, name: &str) -> Option<SlotId> {
        self.slot_by_name
            .get(name)
            .copied()
            .map(|index| SlotId::new(self.key, index))
    }

    /// Resolves a skin name without allocating.
    #[must_use]
    pub fn skin_id(&self, name: &str) -> Option<SkinId> {
        self.skin_by_name
            .get(name)
            .copied()
            .map(|index| SkinId::new(self.key, index))
    }

    /// Resolves an animation name without allocating.
    #[must_use]
    pub fn animation_id(&self, name: &str) -> Option<AnimationId> {
        self.animation_by_name
            .get(name)
            .copied()
            .map(|index| AnimationId::new(self.key, index))
    }

    /// Resolves an IK constraint name without allocating.
    #[must_use]
    pub fn ik_constraint_id(&self, name: &str) -> Option<IkConstraintId> {
        self.ik_constraint_by_name
            .get(name)
            .copied()
            .map(|index| IkConstraintId::new(self.key, index))
    }

    /// Resolves any authored constraint name without allocating.
    #[must_use]
    pub fn constraint_id(&self, name: &str) -> Option<ConstraintId> {
        self.constraint_by_name
            .get(name)
            .copied()
            .map(|index| ConstraintId::new(self.key, index))
    }

    /// Resolves an event-definition name without allocating.
    #[must_use]
    pub fn event_id(&self, name: &str) -> Option<EventId> {
        self.event_by_name
            .get(name)
            .copied()
            .map(|index| EventId::new(self.key, index))
    }

    /// Resolves an atlas page name without allocating.
    #[must_use]
    pub fn atlas_page_id(&self, name: &str) -> Option<AtlasPageId> {
        self.atlas_page_by_name
            .get(name)
            .copied()
            .map(|index| AtlasPageId::new(self.key, index))
    }

    /// Borrows one bone after validating its asset identity.
    pub fn bone(&self, id: BoneId) -> Result<BoneRef<'_>, IdError> {
        let index = self.bone_index(id)?;
        Ok(BoneRef { asset: self, index })
    }

    /// Borrows one slot after validating its asset identity.
    pub fn slot(&self, id: SlotId) -> Result<SlotRef<'_>, IdError> {
        let index = self.checked_index(id.asset(), id.index(), self.slots.len())?;
        Ok(SlotRef { asset: self, index })
    }

    /// Borrows one skin after validating its asset identity.
    pub fn skin(&self, id: SkinId) -> Result<SkinRef<'_>, IdError> {
        let index = self.checked_index(id.asset(), id.index(), self.skins.len())?;
        Ok(SkinRef { asset: self, index })
    }

    /// Borrows one attachment after validating its asset identity.
    pub fn attachment(&self, id: AttachmentId) -> Result<AttachmentRef<'_>, IdError> {
        let index = self.checked_index(id.asset(), id.index(), self.attachments.len())?;
        Ok(AttachmentRef { asset: self, index })
    }

    /// Borrows one animation after validating its asset identity.
    pub fn animation(&self, id: AnimationId) -> Result<AnimationRef<'_>, IdError> {
        let index = self.checked_index(id.asset(), id.index(), self.animations.len())?;
        Ok(AnimationRef { asset: self, index })
    }

    /// Borrows one IK constraint after validating its asset identity.
    pub fn ik_constraint(&self, id: IkConstraintId) -> Result<IkConstraintRef<'_>, IdError> {
        let index = self.checked_index(id.asset(), id.index(), self.ik_constraints.len())?;
        Ok(IkConstraintRef { asset: self, index })
    }

    /// Borrows one authored constraint after validating its asset identity.
    pub fn constraint(&self, id: ConstraintId) -> Result<ConstraintRef<'_>, IdError> {
        let index = self.checked_index(id.asset(), id.index(), self.constraints.len())?;
        Ok(ConstraintRef { asset: self, index })
    }

    /// Borrows one event definition after validating its asset identity.
    pub fn event_definition(&self, id: EventId) -> Result<EventDefinitionRef<'_>, IdError> {
        let index = self.checked_index(id.asset(), id.index(), self.events.len())?;
        Ok(EventDefinitionRef { asset: self, index })
    }

    /// Borrows one atlas page after validating its asset identity.
    pub fn atlas_page(&self, id: AtlasPageId) -> Result<AtlasPageRef<'_>, IdError> {
        let index = self.checked_index(id.asset(), id.index(), self.atlas_pages.len())?;
        Ok(AtlasPageRef { asset: self, index })
    }

    /// Borrows one atlas region after validating its asset identity.
    pub fn atlas_region(&self, id: AtlasRegionId) -> Result<AtlasRegionRef<'_>, IdError> {
        let index = self.checked_index(id.asset(), id.index(), self.atlas_regions.len())?;
        Ok(AtlasRegionRef { asset: self, index })
    }

    /// Iterates bones in source order.
    pub fn bones(&self) -> impl DoubleEndedIterator<Item = BoneRef<'_>> + ExactSizeIterator + '_ {
        (0..self.bones.len()).map(|index| BoneRef { asset: self, index })
    }

    /// Iterates slots in setup-pose draw order.
    pub fn slots(&self) -> impl DoubleEndedIterator<Item = SlotRef<'_>> + ExactSizeIterator + '_ {
        (0..self.slots.len()).map(|index| SlotRef { asset: self, index })
    }

    /// Iterates skins in source order.
    pub fn skins(&self) -> impl DoubleEndedIterator<Item = SkinRef<'_>> + ExactSizeIterator + '_ {
        (0..self.skins.len()).map(|index| SkinRef { asset: self, index })
    }

    /// Iterates attachments in skin, slot, and source order.
    pub fn attachments(
        &self,
    ) -> impl DoubleEndedIterator<Item = AttachmentRef<'_>> + ExactSizeIterator + '_ {
        (0..self.attachments.len()).map(|index| AttachmentRef { asset: self, index })
    }

    /// Iterates animations in source order.
    pub fn animations(
        &self,
    ) -> impl DoubleEndedIterator<Item = AnimationRef<'_>> + ExactSizeIterator + '_ {
        (0..self.animations.len()).map(|index| AnimationRef { asset: self, index })
    }

    /// Iterates IK constraints in authored evaluation order.
    pub fn ik_constraints(
        &self,
    ) -> impl DoubleEndedIterator<Item = IkConstraintRef<'_>> + ExactSizeIterator + '_ {
        (0..self.ik_constraints.len()).map(|index| IkConstraintRef { asset: self, index })
    }

    /// Iterates every authored constraint in source order.
    pub fn constraints(
        &self,
    ) -> impl DoubleEndedIterator<Item = ConstraintRef<'_>> + ExactSizeIterator + '_ {
        (0..self.constraints.len()).map(|index| ConstraintRef { asset: self, index })
    }

    /// Iterates event definitions in source order.
    pub fn event_definitions(
        &self,
    ) -> impl DoubleEndedIterator<Item = EventDefinitionRef<'_>> + ExactSizeIterator + '_ {
        (0..self.events.len()).map(|index| EventDefinitionRef { asset: self, index })
    }

    /// Iterates texture-atlas pages in source order.
    pub fn atlas_pages(
        &self,
    ) -> impl DoubleEndedIterator<Item = AtlasPageRef<'_>> + ExactSizeIterator + '_ {
        (0..self.atlas_pages.len()).map(|index| AtlasPageRef { asset: self, index })
    }

    /// Iterates texture-atlas regions in source order.
    pub fn atlas_regions(
        &self,
    ) -> impl DoubleEndedIterator<Item = AtlasRegionRef<'_>> + ExactSizeIterator + '_ {
        (0..self.atlas_regions.len()).map(|index| AtlasRegionRef { asset: self, index })
    }

    /// Iterates every atlas region with the given name without allocating.
    pub fn atlas_regions_named<'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = AtlasRegionRef<'a>> + 'a {
        self.atlas_regions_by_name
            .get(name)
            .into_iter()
            .flat_map(|indexes| indexes.iter().copied())
            .map(|index| AtlasRegionRef {
                asset: self,
                index: index as usize,
            })
    }

    /// Returns non-fatal issues retained from loading and linking.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Returns whether any retained diagnostic changes visible or behavioral
    /// output.
    #[must_use]
    pub fn has_degradations(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_degraded)
    }

    pub(crate) fn bone_index(&self, id: BoneId) -> Result<usize, IdError> {
        self.checked_index(id.asset(), id.index(), self.bones.len())
    }

    pub(crate) fn bone_data(&self, index: usize) -> &BoneData {
        &self.bones[index]
    }

    #[allow(
        dead_code,
        reason = "Stage 3 evaluates the typed timeline payloads retained by Stage 2"
    )]
    pub(crate) fn animation_data(&self, index: usize) -> &AnimationData {
        &self.animations[index]
    }

    pub(crate) const fn key(&self) -> AssetKey {
        self.key
    }

    fn checked_index(&self, asset: AssetKey, index: u32, len: usize) -> Result<usize, IdError> {
        if asset != self.key {
            return Err(IdError::foreign_asset());
        }

        let index = index as usize;
        assert!(
            index < len,
            "Spinal emitted an out-of-bounds ID for an immutable asset"
        );
        Ok(index)
    }

    #[cfg(test)]
    pub(crate) fn test_fixture(label: &str) -> Self {
        let key = AssetKey::try_fresh().expect("test process has an asset identity available");
        let bones = vec![
            BoneData {
                name: format!("{label}-root").into_boxed_str(),
                parent: None,
                length: 0.0,
                setup_transform: BoneTransform::IDENTITY,
            },
            BoneData {
                name: format!("{label}-head").into_boxed_str(),
                parent: Some(0),
                length: 0.0,
                setup_transform: BoneTransform::IDENTITY,
            },
        ]
        .into_boxed_slice();
        let slots = ["body", "eyes"]
            .into_iter()
            .map(|name| SlotData {
                name: name.into(),
                bone: 0,
                setup_attachment: None,
                colour: Rgba8::WHITE,
                blend_mode: SlotBlendMode::Normal,
                blend_token: "normal".into(),
            })
            .collect();
        let skins = ["default", "blue"]
            .into_iter()
            .map(|name| SkinData {
                name: name.into(),
                attachments: 0..0,
            })
            .collect();
        let animations = vec![AnimationData {
            name: "idle".into(),
            duration: Duration::from_millis(750),
            timelines: Box::default(),
        }]
        .into_boxed_slice();
        let ik_constraints = vec![IkConstraintData {
            constraint: 0,
            name: "look".into(),
            order: 0,
            bones: vec![0].into_boxed_slice(),
            target: 1,
            mix: Mix::ONE,
            bend_direction: BendDirection::Positive,
            softness: 0.0,
            compress: false,
            stretch: false,
            uniform: false,
        }]
        .into_boxed_slice();

        Self::from_data(
            key,
            AssetData {
                spine_version: "4.3.23".into(),
                bones,
                slots,
                skins,
                attachments: Box::default(),
                animations,
                ik_constraints,
                constraints: vec![ConstraintData {
                    name: "look".into(),
                    source_type: "ik".into(),
                    order: 0,
                    ik_constraint: Some(0),
                }]
                .into_boxed_slice(),
                atlas_pages: Box::default(),
                atlas_regions: Box::default(),
                events: Box::default(),
                diagnostics: Box::default(),
            },
        )
    }
}

/// A borrowed immutable bone definition.
#[derive(Clone, Copy, Debug)]
pub struct BoneRef<'a> {
    asset: &'a SkeletonAsset,
    index: usize,
}

impl<'a> BoneRef<'a> {
    /// Returns the asset-scoped bone ID.
    #[must_use]
    pub fn id(self) -> BoneId {
        BoneId::new(self.asset.key, self.index as u32)
    }

    /// Returns the bone name.
    #[must_use]
    pub fn name(self) -> &'a str {
        &self.asset.bones[self.index].name
    }

    /// Returns the parent bone, if any.
    #[must_use]
    pub fn parent(self) -> Option<BoneId> {
        self.asset.bones[self.index]
            .parent
            .map(|index| BoneId::new(self.asset.key, index))
    }

    /// Returns the setup-pose bone length.
    #[must_use]
    pub fn length(self) -> f32 {
        self.asset.bones[self.index].length
    }

    /// Returns the setup-pose local transform.
    #[must_use]
    pub fn setup_transform(self) -> BoneTransform {
        self.asset.bones[self.index].setup_transform
    }

    /// Returns the source-order position of this bone.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.index
    }
}

/// A borrowed immutable slot definition.
#[derive(Clone, Copy, Debug)]
pub struct SlotRef<'a> {
    asset: &'a SkeletonAsset,
    index: usize,
}

impl<'a> SlotRef<'a> {
    /// Returns the asset-scoped slot ID.
    #[must_use]
    pub fn id(self) -> SlotId {
        SlotId::new(self.asset.key, self.index as u32)
    }

    /// Returns the authored slot name.
    #[must_use]
    pub fn name(self) -> &'a str {
        &self.asset.slots[self.index].name
    }

    /// Returns the bone that owns this slot.
    #[must_use]
    pub fn bone(self) -> BoneId {
        BoneId::new(self.asset.key, self.asset.slots[self.index].bone)
    }

    /// Returns the linked default-skin setup attachment, if any.
    #[must_use]
    pub fn setup_attachment(self) -> Option<AttachmentId> {
        self.asset.slots[self.index]
            .setup_attachment
            .map(|index| AttachmentId::new(self.asset.key, index))
    }

    /// Returns the setup light colour.
    #[must_use]
    pub fn color(self) -> Rgba8 {
        self.asset.slots[self.index].colour
    }

    /// Returns the authored blend mode.
    #[must_use]
    pub fn blend_mode(self) -> SlotBlendMode {
        self.asset.slots[self.index].blend_mode
    }

    /// Returns the exact authored blend-mode token.
    #[must_use]
    pub fn blend_token(self) -> &'a str {
        &self.asset.slots[self.index].blend_token
    }

    /// Returns the setup draw-order position.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.index
    }
}

/// A borrowed immutable skin definition.
#[derive(Clone, Copy, Debug)]
pub struct SkinRef<'a> {
    asset: &'a SkeletonAsset,
    index: usize,
}

impl<'a> SkinRef<'a> {
    /// Returns the asset-scoped skin ID.
    #[must_use]
    pub fn id(self) -> SkinId {
        SkinId::new(self.asset.key, self.index as u32)
    }

    /// Returns the authored skin name.
    #[must_use]
    pub fn name(self) -> &'a str {
        &self.asset.skins[self.index].name
    }

    /// Iterates this skin's attachments in slot and source order.
    pub fn attachments(self) -> impl DoubleEndedIterator<Item = AttachmentRef<'a>> + 'a {
        let range = self.asset.skins[self.index].attachments.clone();
        range.map(|index| AttachmentRef {
            asset: self.asset,
            index: index as usize,
        })
    }

    /// Finds an attachment authored directly in this skin without allocating.
    ///
    /// This exact-skin lookup does not fall back to the default skin. Runtime
    /// skin composition and fallback are added in Stage 3.
    pub fn attachment(self, slot: SlotId, name: &str) -> Result<Option<AttachmentId>, IdError> {
        let slot_index =
            self.asset
                .checked_index(slot.asset(), slot.index(), self.asset.slots.len())?
                as u32;
        Ok(self
            .asset
            .attachment_by_skin_slot
            .get(&(self.index as u32, slot_index))
            .and_then(|attachments| attachments.get(name))
            .copied()
            .map(|index| AttachmentId::new(self.asset.key, index)))
    }

    /// Returns the source-order position.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.index
    }
}

/// A borrowed immutable attachment definition.
#[derive(Clone, Copy, Debug)]
pub struct AttachmentRef<'a> {
    asset: &'a SkeletonAsset,
    index: usize,
}

impl<'a> AttachmentRef<'a> {
    /// Returns the asset-scoped attachment ID.
    #[must_use]
    pub fn id(self) -> AttachmentId {
        AttachmentId::new(self.asset.key, self.index as u32)
    }

    /// Returns the actual authored attachment name.
    #[must_use]
    pub fn name(self) -> &'a str {
        &self.asset.attachments[self.index].name
    }

    /// Returns the skin-local placeholder used by slots and attachment keys.
    #[must_use]
    pub fn placeholder_name(self) -> &'a str {
        &self.asset.attachments[self.index].placeholder_name
    }

    /// Returns the explicitly authored atlas path, if any.
    #[must_use]
    pub fn atlas_path(self) -> Option<&'a str> {
        self.asset.attachments[self.index].atlas_path.as_deref()
    }

    /// Returns the skin containing this attachment.
    #[must_use]
    pub fn skin(self) -> SkinId {
        SkinId::new(self.asset.key, self.asset.attachments[self.index].skin)
    }

    /// Returns the slot containing this attachment.
    #[must_use]
    pub fn slot(self) -> SlotId {
        SlotId::new(self.asset.key, self.asset.attachments[self.index].slot)
    }

    /// Returns the retained attachment category.
    #[must_use]
    pub fn kind(self) -> AttachmentKind {
        match &self.asset.attachments[self.index].kind {
            AttachmentDataKind::Region(_) => AttachmentKind::Region,
            AttachmentDataKind::BoundingBox => AttachmentKind::BoundingBox,
            AttachmentDataKind::Point => AttachmentKind::Point,
            AttachmentDataKind::Unsupported { .. } => AttachmentKind::Unsupported,
        }
    }

    /// Returns the original unsupported attachment token, when applicable.
    #[must_use]
    pub fn unsupported_type(self) -> Option<&'a str> {
        match &self.asset.attachments[self.index].kind {
            AttachmentDataKind::Unsupported { source_type } => Some(source_type),
            _ => None,
        }
    }

    /// Returns a typed rigid-region view, when this is a region attachment.
    #[must_use]
    pub fn as_region(self) -> Option<RegionAttachmentRef<'a>> {
        self.region()
            .map(|_region| RegionAttachmentRef { attachment: self })
    }

    /// Returns the source-order position.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.index
    }

    fn region(self) -> Option<&'a RegionAttachmentData> {
        match &self.asset.attachments[self.index].kind {
            AttachmentDataKind::Region(region) => Some(region),
            _ => None,
        }
    }
}

/// A typed borrowed view of one rigid textured region attachment.
#[derive(Clone, Copy, Debug)]
pub struct RegionAttachmentRef<'a> {
    attachment: AttachmentRef<'a>,
}

impl<'a> RegionAttachmentRef<'a> {
    /// Returns the attachment that owns this region payload.
    #[must_use]
    pub const fn attachment(self) -> AttachmentRef<'a> {
        self.attachment
    }

    /// Returns the region's setup-pose local transform.
    #[must_use]
    pub fn local_transform(self) -> BoneTransform {
        self.data().transform
    }

    /// Returns the region's original authored extent.
    #[must_use]
    pub fn size(self) -> PixelSize {
        self.data().size
    }

    /// Returns the authored light colour.
    #[must_use]
    pub fn color(self) -> Rgba8 {
        self.data().colour
    }

    /// Returns the linked atlas region.
    #[must_use]
    pub fn atlas_region(self) -> AtlasRegionId {
        AtlasRegionId::new(self.attachment.asset.key, self.data().atlas_region)
    }

    fn data(self) -> &'a RegionAttachmentData {
        self.attachment
            .region()
            .expect("RegionAttachmentRef is constructed only for region attachments")
    }
}

/// A borrowed immutable animation definition.
#[derive(Clone, Copy, Debug)]
pub struct AnimationRef<'a> {
    asset: &'a SkeletonAsset,
    index: usize,
}

impl<'a> AnimationRef<'a> {
    /// Returns the asset-scoped animation ID.
    #[must_use]
    pub fn id(self) -> AnimationId {
        AnimationId::new(self.asset.key, self.index as u32)
    }

    /// Returns the authored animation name.
    #[must_use]
    pub fn name(self) -> &'a str {
        &self.asset.animations[self.index].name
    }

    /// Returns the animation duration.
    #[must_use]
    pub fn duration(self) -> Duration {
        self.asset.animations[self.index].duration
    }

    /// Returns the source-order position.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.index
    }
}

/// A borrowed immutable one- or two-bone IK constraint.
#[derive(Clone, Copy, Debug)]
pub struct IkConstraintRef<'a> {
    asset: &'a SkeletonAsset,
    index: usize,
}

impl<'a> IkConstraintRef<'a> {
    /// Returns the asset-scoped IK constraint ID.
    #[must_use]
    pub fn id(self) -> IkConstraintId {
        IkConstraintId::new(self.asset.key, self.index as u32)
    }

    /// Returns the corresponding record in the complete constraint table.
    #[must_use]
    pub fn constraint(self) -> ConstraintRef<'a> {
        ConstraintRef {
            asset: self.asset,
            index: self.asset.ik_constraints[self.index].constraint as usize,
        }
    }

    /// Returns the authored name.
    #[must_use]
    pub fn name(self) -> &'a str {
        &self.asset.ik_constraints[self.index].name
    }

    /// Returns the global constraint evaluation order.
    #[must_use]
    pub fn order(self) -> u32 {
        self.asset.ik_constraints[self.index].order
    }

    /// Iterates the constrained bones in chain order.
    pub fn bones(self) -> impl DoubleEndedIterator<Item = BoneId> + ExactSizeIterator + 'a {
        self.asset.ik_constraints[self.index]
            .bones
            .iter()
            .copied()
            .map(|index| BoneId::new(self.asset.key, index))
    }

    /// Returns the target bone.
    #[must_use]
    pub fn target(self) -> BoneId {
        BoneId::new(self.asset.key, self.asset.ik_constraints[self.index].target)
    }

    /// Returns the setup influence.
    #[must_use]
    pub fn mix(self) -> Mix {
        self.asset.ik_constraints[self.index].mix
    }

    /// Returns the setup bend direction.
    #[must_use]
    pub fn bend_direction(self) -> BendDirection {
        self.asset.ik_constraints[self.index].bend_direction
    }

    /// Returns the authored softness.
    #[must_use]
    pub fn softness(self) -> f32 {
        self.asset.ik_constraints[self.index].softness
    }

    /// Returns whether compression was authored.
    #[must_use]
    pub fn compress(self) -> bool {
        self.asset.ik_constraints[self.index].compress
    }

    /// Returns whether stretching was authored.
    #[must_use]
    pub fn stretch(self) -> bool {
        self.asset.ik_constraints[self.index].stretch
    }

    /// Returns whether uniform scaling was authored.
    #[must_use]
    pub fn uniform(self) -> bool {
        self.asset.ik_constraints[self.index].uniform
    }

    /// Returns the evaluation-order position among supported IK constraints.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.index
    }
}

/// A borrowed immutable authored constraint record.
///
/// Unsupported constraint types remain visible here so tooling can explain
/// degraded content without interpreting their private payload.
#[derive(Clone, Copy, Debug)]
pub struct ConstraintRef<'a> {
    asset: &'a SkeletonAsset,
    index: usize,
}

impl<'a> ConstraintRef<'a> {
    /// Returns the asset-scoped constraint ID.
    #[must_use]
    pub fn id(self) -> ConstraintId {
        ConstraintId::new(self.asset.key, self.index as u32)
    }

    /// Returns the authored constraint name.
    #[must_use]
    pub fn name(self) -> &'a str {
        &self.asset.constraints[self.index].name
    }

    /// Returns the authored constraint type token.
    #[must_use]
    pub fn source_type(self) -> &'a str {
        &self.asset.constraints[self.index].source_type
    }

    /// Returns the global evaluation order.
    #[must_use]
    pub fn order(self) -> u32 {
        self.asset.constraints[self.index].order
    }

    /// Returns a typed IK view when this is a supported IK constraint.
    #[must_use]
    pub fn as_ik(self) -> Option<IkConstraintRef<'a>> {
        self.asset.constraints[self.index]
            .ik_constraint
            .map(|index| IkConstraintRef {
                asset: self.asset,
                index: index as usize,
            })
    }

    /// Returns the source-order position.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.index
    }
}

/// A borrowed immutable event definition.
#[derive(Clone, Copy, Debug)]
pub struct EventDefinitionRef<'a> {
    asset: &'a SkeletonAsset,
    index: usize,
}

impl<'a> EventDefinitionRef<'a> {
    /// Returns the asset-scoped event ID.
    #[must_use]
    pub fn id(self) -> EventId {
        EventId::new(self.asset.key, self.index as u32)
    }

    /// Returns the authored event name.
    #[must_use]
    pub fn name(self) -> &'a str {
        &self.asset.events[self.index].name
    }

    /// Returns the default integer payload.
    #[must_use]
    pub fn integer(self) -> i32 {
        self.asset.events[self.index].payload.integer
    }

    /// Returns the default floating-point payload.
    #[must_use]
    pub fn float(self) -> f32 {
        self.asset.events[self.index].payload.float
    }

    /// Returns the default string payload.
    #[must_use]
    pub fn string(self) -> Option<&'a str> {
        self.asset.events[self.index].payload.string.as_deref()
    }

    /// Returns the optional authored audio path.
    #[must_use]
    pub fn audio(self) -> Option<&'a str> {
        self.asset.events[self.index].audio.as_deref()
    }

    /// Returns the default audio volume.
    #[must_use]
    pub fn volume(self) -> f32 {
        self.asset.events[self.index].payload.volume
    }

    /// Returns the default audio balance.
    #[must_use]
    pub fn balance(self) -> f32 {
        self.asset.events[self.index].payload.balance
    }

    /// Returns the source-order position.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.index
    }
}

/// One retained extension property from a text atlas.
#[derive(Clone, Copy, Debug)]
pub struct AtlasPropertyRef<'a> {
    property: &'a AtlasExtension,
}

impl<'a> AtlasPropertyRef<'a> {
    /// Returns the property key.
    #[must_use]
    pub fn key(self) -> &'a str {
        &self.property.key
    }

    /// Returns the property value exactly as parsed after outer whitespace.
    #[must_use]
    pub fn value(self) -> &'a str {
        &self.property.value
    }
}

/// A borrowed immutable texture-atlas page.
#[derive(Clone, Copy, Debug)]
pub struct AtlasPageRef<'a> {
    asset: &'a SkeletonAsset,
    index: usize,
}

impl<'a> AtlasPageRef<'a> {
    /// Returns the asset-scoped page ID.
    #[must_use]
    pub fn id(self) -> AtlasPageId {
        AtlasPageId::new(self.asset.key, self.index as u32)
    }

    /// Returns the page image name exactly as authored.
    #[must_use]
    pub fn name(self) -> &'a str {
        &self.asset.atlas_pages[self.index].name
    }

    /// Returns the declared page size, or zeroes when omitted.
    #[must_use]
    pub fn size(self) -> PixelSize {
        self.asset.atlas_pages[self.index].size
    }

    /// Returns the documented texture format classification.
    #[must_use]
    pub fn format(self) -> TextureFormat {
        self.asset.atlas_pages[self.index].format
    }

    /// Returns the original texture format token.
    #[must_use]
    pub fn format_token(self) -> &'a str {
        &self.asset.atlas_pages[self.index].format_token
    }

    /// Returns the minification filter.
    #[must_use]
    pub fn min_filter(self) -> TextureFilter {
        self.asset.atlas_pages[self.index].min_filter
    }

    /// Returns the original minification filter token.
    #[must_use]
    pub fn min_filter_token(self) -> &'a str {
        &self.asset.atlas_pages[self.index].min_filter_token
    }

    /// Returns the magnification filter.
    #[must_use]
    pub fn mag_filter(self) -> TextureFilter {
        self.asset.atlas_pages[self.index].mag_filter
    }

    /// Returns the original magnification filter token.
    #[must_use]
    pub fn mag_filter_token(self) -> &'a str {
        &self.asset.atlas_pages[self.index].mag_filter_token
    }

    /// Returns the page wrap mode.
    #[must_use]
    pub fn wrap(self) -> WrapMode {
        self.asset.atlas_pages[self.index].wrap
    }

    /// Returns the page alpha encoding.
    #[must_use]
    pub fn alpha_encoding(self) -> AlphaEncoding {
        self.asset.atlas_pages[self.index].alpha_encoding
    }

    /// Returns the atlas export scale.
    #[must_use]
    pub fn scale(self) -> f32 {
        self.asset.atlas_pages[self.index].scale
    }

    /// Iterates this page's atlas regions in source order.
    pub fn regions(self) -> impl DoubleEndedIterator<Item = AtlasRegionRef<'a>> + 'a {
        let range = self.asset.atlas_pages[self.index].regions.clone();
        range.map(|index| AtlasRegionRef {
            asset: self.asset,
            index: index as usize,
        })
    }

    /// Iterates unknown or extended page properties in source order.
    pub fn extensions(
        self,
    ) -> impl DoubleEndedIterator<Item = AtlasPropertyRef<'a>> + ExactSizeIterator + 'a {
        self.asset.atlas_pages[self.index]
            .extensions
            .iter()
            .map(|property| AtlasPropertyRef { property })
    }

    /// Returns the source-order position.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.index
    }
}

/// A borrowed immutable texture-atlas region.
#[derive(Clone, Copy, Debug)]
pub struct AtlasRegionRef<'a> {
    asset: &'a SkeletonAsset,
    index: usize,
}

impl<'a> AtlasRegionRef<'a> {
    /// Returns the asset-scoped region ID.
    #[must_use]
    pub fn id(self) -> AtlasRegionId {
        AtlasRegionId::new(self.asset.key, self.index as u32)
    }

    /// Returns the authored region name.
    #[must_use]
    pub fn name(self) -> &'a str {
        &self.asset.atlas_regions[self.index].name
    }

    /// Returns the containing page.
    #[must_use]
    pub fn page(self) -> AtlasPageId {
        AtlasPageId::new(self.asset.key, self.asset.atlas_regions[self.index].page)
    }

    /// Returns the optional sequence index.
    #[must_use]
    pub fn index(self) -> Option<u32> {
        self.asset.atlas_regions[self.index].index
    }

    /// Returns the packed page-space bounds.
    #[must_use]
    pub fn bounds(self) -> PixelRect {
        self.asset.atlas_regions[self.index].bounds
    }

    /// Returns the unpacked trimming metadata.
    #[must_use]
    pub fn trim(self) -> Trim {
        self.asset.atlas_regions[self.index].trim
    }

    /// Returns the authored packed rotation.
    #[must_use]
    pub fn rotation(self) -> AtlasRotation {
        self.asset.atlas_regions[self.index].rotation
    }

    /// Returns optional nine-patch splits.
    #[must_use]
    pub fn split(self) -> Option<[i32; 4]> {
        self.asset.atlas_regions[self.index].split
    }

    /// Returns optional nine-patch padding.
    #[must_use]
    pub fn pad(self) -> Option<[i32; 4]> {
        self.asset.atlas_regions[self.index].pad
    }

    /// Iterates extended region properties in source order.
    pub fn extensions(
        self,
    ) -> impl DoubleEndedIterator<Item = AtlasPropertyRef<'a>> + ExactSizeIterator + 'a {
        self.asset.atlas_regions[self.index]
            .extensions
            .iter()
            .map(|property| AtlasPropertyRef { property })
    }

    /// Returns the source-order position.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.index
    }
}

fn lookup<T>(values: &[T], name: impl Fn(&T) -> &str) -> HashMap<Box<str>, u32> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            (
                Box::from(name(value)),
                u32::try_from(index).expect("validated asset tables fit u32 indexes"),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{DiagnosticCode, DiagnosticScope, DiagnosticSeverity, IdErrorKind};

    #[test]
    fn iteration_and_name_lookup_preserve_source_order() {
        let asset = SkeletonAsset::test_fixture("cat");

        let names: Vec<_> = asset.bones().map(BoneRef::name).collect();
        assert_eq!(names, ["cat-root", "cat-head"]);

        let head_id = asset.bone_id("cat-head").expect("head exists");
        let head = asset.bone(head_id).expect("ID belongs to this asset");
        assert_eq!(head.ordinal(), 1);
        assert_eq!(
            head.parent().expect("head has a parent"),
            asset.bone_id("cat-root").expect("root exists")
        );

        let animation = asset
            .animation(asset.animation_id("idle").expect("idle exists"))
            .expect("ID belongs to this asset");
        assert_eq!(animation.duration(), Duration::from_millis(750));

        assert_eq!(
            asset.slots().map(SlotRef::name).collect::<Vec<_>>(),
            ["body", "eyes"]
        );
        assert_eq!(
            asset.skins().map(SkinRef::name).collect::<Vec<_>>(),
            ["default", "blue"]
        );
        assert_eq!(
            asset
                .ik_constraints()
                .map(IkConstraintRef::name)
                .collect::<Vec<_>>(),
            ["look"]
        );
    }

    #[test]
    fn ids_cannot_cross_asset_boundaries() {
        let first = SkeletonAsset::test_fixture("first");
        let second = SkeletonAsset::test_fixture("second");
        let foreign_id = first.bone_id("first-root").expect("root exists");

        let error = second
            .bone(foreign_id)
            .expect_err("foreign IDs must be rejected");
        assert_eq!(error.kind(), IdErrorKind::ForeignAsset);
    }

    #[test]
    fn assets_are_shared_by_arc_instead_of_cloned() {
        let asset = Arc::new(SkeletonAsset::test_fixture("cat"));
        let shared = Arc::clone(&asset);
        assert!(Arc::ptr_eq(&asset, &shared));
    }

    #[test]
    fn assets_retain_structured_degradation_diagnostics() {
        let mut asset = SkeletonAsset::test_fixture("cat");
        asset.diagnostics = vec![Diagnostic {
            severity: DiagnosticSeverity::Degraded,
            code: DiagnosticCode::UnsupportedAttachmentType,
            scope: DiagnosticScope::Asset,
            message: "mesh attachment was ignored".into(),
        }]
        .into_boxed_slice();

        assert!(asset.has_degradations());
        assert_eq!(asset.diagnostics().len(), 1);
        assert_eq!(
            asset.diagnostics()[0].code(),
            DiagnosticCode::UnsupportedAttachmentType
        );
        assert_eq!(asset.diagnostics()[0].scope(), DiagnosticScope::Asset);
        assert_eq!(
            asset.diagnostics()[0].message(),
            "mesh attachment was ignored"
        );
    }
}

use std::{collections::HashMap, time::Duration};

use crate::{
    AnimationId, BoneId, BoneTransform, Diagnostic, IdError, IkConstraintId, SkinId, SlotId,
    id::AssetKey,
};

#[derive(Debug)]
pub(crate) struct BoneData {
    pub(crate) name: Box<str>,
    pub(crate) parent: Option<u32>,
    pub(crate) setup_transform: BoneTransform,
}

#[derive(Debug)]
struct NamedData {
    name: Box<str>,
}

#[derive(Debug)]
struct AnimationData {
    name: Box<str>,
    duration: Duration,
}

/// Immutable, linked skeleton data shared by runtime instances.
///
/// A loaded asset is intentionally not `Clone`. Share ownership through
/// [`std::sync::Arc`] so every runtime instance retains the identity used by
/// its typed IDs.
#[derive(Debug)]
pub struct SkeletonAsset {
    key: AssetKey,
    bones: Box<[BoneData]>,
    slots: Box<[NamedData]>,
    skins: Box<[NamedData]>,
    animations: Box<[AnimationData]>,
    ik_constraints: Box<[NamedData]>,
    bone_by_name: HashMap<Box<str>, u32>,
    slot_by_name: HashMap<Box<str>, u32>,
    skin_by_name: HashMap<Box<str>, u32>,
    animation_by_name: HashMap<Box<str>, u32>,
    ik_constraint_by_name: HashMap<Box<str>, u32>,
    diagnostics: Box<[Diagnostic]>,
}

impl SkeletonAsset {
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

    /// Iterates animations in source order.
    pub fn animations(
        &self,
    ) -> impl DoubleEndedIterator<Item = AnimationRef<'_>> + ExactSizeIterator + '_ {
        (0..self.animations.len()).map(|index| AnimationRef { asset: self, index })
    }

    /// Iterates IK constraints in their exported evaluation order.
    pub fn ik_constraints(
        &self,
    ) -> impl DoubleEndedIterator<Item = IkConstraintRef<'_>> + ExactSizeIterator + '_ {
        (0..self.ik_constraints.len()).map(|index| IkConstraintRef { asset: self, index })
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
        let key = AssetKey::fresh();
        let bones = vec![
            BoneData {
                name: format!("{label}-root").into_boxed_str(),
                parent: None,
                setup_transform: BoneTransform::IDENTITY,
            },
            BoneData {
                name: format!("{label}-head").into_boxed_str(),
                parent: Some(0),
                setup_transform: BoneTransform::IDENTITY,
            },
        ]
        .into_boxed_slice();
        let slots = named_data(["body", "eyes"]);
        let skins = named_data(["default", "blue"]);
        let animations = vec![AnimationData {
            name: "idle".into(),
            duration: Duration::from_millis(750),
        }]
        .into_boxed_slice();
        let ik_constraints = named_data(["look"]);

        Self {
            key,
            bone_by_name: lookup(&bones, |bone| &bone.name),
            slot_by_name: lookup(&slots, |slot| &slot.name),
            skin_by_name: lookup(&skins, |skin| &skin.name),
            animation_by_name: lookup(&animations, |animation| &animation.name),
            ik_constraint_by_name: lookup(&ik_constraints, |ik| &ik.name),
            bones,
            slots,
            skins,
            animations,
            ik_constraints,
            diagnostics: Box::default(),
        }
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

macro_rules! named_ref {
    (
        $name:ident,
        $id:ident,
        $field:ident,
        $documentation:literal
    ) => {
        #[doc = $documentation]
        #[derive(Clone, Copy, Debug)]
        pub struct $name<'a> {
            asset: &'a SkeletonAsset,
            index: usize,
        }

        impl<'a> $name<'a> {
            /// Returns the asset-scoped identifier.
            #[must_use]
            pub fn id(self) -> $id {
                $id::new(self.asset.key, self.index as u32)
            }

            /// Returns the authored name.
            #[must_use]
            pub fn name(self) -> &'a str {
                &self.asset.$field[self.index].name
            }

            /// Returns the source-order position.
            #[must_use]
            pub const fn ordinal(self) -> usize {
                self.index
            }
        }
    };
}

named_ref!(
    SlotRef,
    SlotId,
    slots,
    "A borrowed immutable slot definition."
);
named_ref!(
    SkinRef,
    SkinId,
    skins,
    "A borrowed immutable skin definition."
);
named_ref!(
    IkConstraintRef,
    IkConstraintId,
    ik_constraints,
    "A borrowed immutable IK constraint definition."
);

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

#[cfg(test)]
fn named_data<const N: usize>(names: [&str; N]) -> Box<[NamedData]> {
    names
        .into_iter()
        .map(|name| NamedData { name: name.into() })
        .collect()
}

#[cfg(test)]
fn lookup<T>(values: &[T], name: impl Fn(&T) -> &str) -> HashMap<Box<str>, u32> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| (Box::from(name(value)), index as u32))
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

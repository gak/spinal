use crate::{
    Angle, AtlasPageId, AtlasRegionId, AttachmentId, BendDirection, BoneId, BonePoseRef,
    BoneTransform, ConstraintId, Diagnostic, DiagnosticScope, DrawItemRef, IdError, IkConstraintId,
    IkConstraintPoseRef, Mix, RegionDrawItemRef, Shear, Skeleton, SkinId, SlotId, SlotPoseRef,
    TransformConstraintId, TransformConstraintPoseRef, TransformMix, UpdateReport,
    world::{
        IkReach, OneBoneIkSolution, WorldTransform, normal_local_to_world, shortest_angle_delta,
        solve_one_bone_ik, solve_two_bone_ik, solve_world_rotation,
    },
};

/// A sampled local pose that may receive procedural edits before constraints.
///
/// This state deliberately exposes no world transforms or draw data. Consuming
/// it with [`EditablePose::solve`] applies constraints and produces a
/// [`SolvedFrame`].
#[derive(Debug)]
#[must_use = "apply procedural edits, then call solve to obtain renderer output"]
pub struct EditablePose<'a> {
    skeleton: &'a mut Skeleton,
    report: UpdateReport,
}

impl<'a> EditablePose<'a> {
    pub(crate) const fn new(skeleton: &'a mut Skeleton, report: UpdateReport) -> Self {
        Self { skeleton, report }
    }

    /// Returns the lifecycle facts from the player update that produced this
    /// pose.
    #[must_use]
    pub const fn report(&self) -> UpdateReport {
        self.report
    }

    /// Opens a short-lived procedural editing view.
    ///
    /// Edits operate in bone-local space and are applied after animation
    /// sampling and crossfading, but before world transforms and IK.
    pub fn edit(&mut self) -> PoseEditor<'_> {
        PoseEditor {
            skeleton: self.skeleton,
        }
    }

    /// Applies world transforms and all supported constraints in authored
    /// evaluation order.
    pub fn solve(self) -> SolvedFrame<'a> {
        solve_world_and_constraints(self.skeleton);
        SolvedFrame {
            skeleton: self.skeleton,
            report: self.report,
        }
    }
}

impl Skeleton {
    /// Opens the current local pose for procedural edits and constraint
    /// solving without using an [`crate::AnimationPlayer`].
    ///
    /// This is the standalone path after [`Skeleton::sample_animation`] or
    /// [`Skeleton::reset_to_setup_pose`]. The resulting update report is
    /// empty because no player clock was advanced.
    pub fn editable_pose(&mut self) -> EditablePose<'_> {
        EditablePose::new(self, UpdateReport::default())
    }
}

/// A scoped procedural editing view over an unsolved local pose.
#[derive(Debug)]
pub struct PoseEditor<'a> {
    skeleton: &'a mut Skeleton,
}

impl PoseEditor<'_> {
    /// Returns one bone's current local transform.
    pub fn bone_local(&self, bone: BoneId) -> Result<BoneTransform, IdError> {
        self.skeleton
            .bone_pose(bone)
            .map(BonePoseRef::local_transform)
    }

    /// Replaces one bone's local transform.
    pub fn set_bone_local(
        &mut self,
        bone: BoneId,
        transform: BoneTransform,
    ) -> Result<(), IdError> {
        let index = self.skeleton.asset().bone_index(bone)?;
        self.skeleton.pose.bones[index].local_transform = transform;
        Ok(())
    }

    /// Returns one IK constraint's current influence.
    pub fn ik_mix(&self, constraint: IkConstraintId) -> Result<Mix, IdError> {
        self.skeleton
            .ik_constraint_pose(constraint)
            .map(IkConstraintPoseRef::mix)
    }

    /// Replaces one IK constraint's influence.
    pub fn set_ik_mix(&mut self, constraint: IkConstraintId, mix: Mix) -> Result<(), IdError> {
        let index = self.skeleton.asset().ik_constraint_index(constraint)?;
        self.skeleton.pose.ik_constraints[index].mix = mix;
        Ok(())
    }

    /// Returns one IK constraint's current bend direction.
    pub fn ik_bend_direction(&self, constraint: IkConstraintId) -> Result<BendDirection, IdError> {
        self.skeleton
            .ik_constraint_pose(constraint)
            .map(IkConstraintPoseRef::bend_direction)
    }

    /// Replaces one IK constraint's bend direction.
    pub fn set_ik_bend_direction(
        &mut self,
        constraint: IkConstraintId,
        bend_direction: BendDirection,
    ) -> Result<(), IdError> {
        let index = self.skeleton.asset().ik_constraint_index(constraint)?;
        self.skeleton.pose.ik_constraints[index].bend_direction = bend_direction;
        Ok(())
    }

    /// Returns one transform constraint's current rotation influence.
    pub fn transform_mix_rotate(
        &self,
        constraint: TransformConstraintId,
    ) -> Result<TransformMix, IdError> {
        self.skeleton
            .transform_constraint_pose(constraint)
            .map(TransformConstraintPoseRef::mix_rotate)
    }

    /// Replaces one transform constraint's rotation influence.
    pub fn set_transform_mix_rotate(
        &mut self,
        constraint: TransformConstraintId,
        mix: TransformMix,
    ) -> Result<(), IdError> {
        let index = self
            .skeleton
            .asset()
            .transform_constraint_index(constraint)?;
        self.skeleton.pose.transform_constraints[index].mix_rotate = mix;
        Ok(())
    }
}

/// Whether a full-influence two-bone IK solution can reach its target.
///
/// This classifies the target geometry, not the endpoint of a partially mixed
/// final pose. One-bone IK points toward its target without changing length
/// and therefore has no value of this type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum IkTargetReach {
    /// The authored two-bone chain can reach the target without stretching.
    Reachable,
    /// The target is outside the authored chain's reach and stretching is
    /// disabled.
    BeyondReach,
}

/// Why an active IK constraint could not be applied safely.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum IkSolveIssue {
    /// The target geometry or an inherited transform was singular or
    /// underdetermined, so the finite FK pose was preserved.
    SingularOrUnderdetermined,
}

/// The result of evaluating one IK constraint for a solved frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IkSolveStatus {
    active: bool,
    preserved_underdetermined: bool,
    target_reach: Option<IkTargetReach>,
    child_translation_y_zeroed: bool,
    issue: Option<IkSolveIssue>,
}

/// Why an active transform constraint could not be applied safely.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum TransformSolveIssue {
    /// The source or inherited transform was singular or underdetermined, so
    /// the finite unconstrained rotation was preserved.
    SingularOrUnderdetermined,
}

/// The result of evaluating one transform constraint for a solved frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransformConstraintSolveStatus {
    active: bool,
    issue: Option<TransformSolveIssue>,
}

impl TransformConstraintSolveStatus {
    pub(crate) const INACTIVE: Self = Self {
        active: false,
        issue: None,
    };

    const APPLIED: Self = Self {
        active: true,
        issue: None,
    };

    const fn skipped(issue: TransformSolveIssue) -> Self {
        Self {
            active: true,
            issue: Some(issue),
        }
    }

    /// Returns whether the supported rotation channel had nonzero influence.
    #[must_use]
    pub const fn is_active(self) -> bool {
        self.active
    }

    /// Returns why the constrained rotation was preserved.
    #[must_use]
    pub const fn issue(self) -> Option<TransformSolveIssue> {
        self.issue
    }

    /// Returns whether a runtime safety fallback changed the authored result.
    #[must_use]
    pub const fn is_degraded(self) -> bool {
        self.issue.is_some()
    }
}

impl IkSolveStatus {
    pub(crate) const INACTIVE: Self = Self {
        active: false,
        preserved_underdetermined: false,
        target_reach: None,
        child_translation_y_zeroed: false,
        issue: None,
    };

    const fn applied(
        target_reach: Option<IkTargetReach>,
        child_translation_y_zeroed: bool,
    ) -> Self {
        Self {
            active: true,
            preserved_underdetermined: false,
            target_reach,
            child_translation_y_zeroed,
            issue: None,
        }
    }

    const fn preserved() -> Self {
        Self {
            active: true,
            preserved_underdetermined: true,
            target_reach: None,
            child_translation_y_zeroed: false,
            issue: None,
        }
    }

    const fn skipped(issue: IkSolveIssue) -> Self {
        Self {
            active: true,
            preserved_underdetermined: false,
            target_reach: None,
            child_translation_y_zeroed: false,
            issue: Some(issue),
        }
    }

    /// Returns whether the constraint had nonzero influence.
    #[must_use]
    pub const fn is_active(self) -> bool {
        self.active
    }

    /// Returns whether coincident one-bone target geometry deliberately kept
    /// the finite FK rotation.
    #[must_use]
    pub const fn preserved_underdetermined(self) -> bool {
        self.preserved_underdetermined
    }

    /// Returns the full-influence two-bone target reachability.
    ///
    /// This does not claim that a partially mixed final pose reaches the
    /// target. `None` is used for one-bone constraints, inactive constraints,
    /// deliberately preserved poses, and unsafe geometry.
    #[must_use]
    pub const fn target_reach(self) -> Option<IkTargetReach> {
        self.target_reach
    }

    /// Returns whether the documented two-bone IK rule reset the child's
    /// local Y translation.
    #[must_use]
    pub const fn child_translation_y_was_zeroed(self) -> bool {
        self.child_translation_y_zeroed
    }

    /// Returns a recoverable runtime issue, if the FK pose had to be
    /// preserved.
    #[must_use]
    pub const fn issue(self) -> Option<IkSolveIssue> {
        self.issue
    }

    /// Returns whether this constraint degraded the authored result.
    #[must_use]
    pub const fn is_degraded(self) -> bool {
        self.issue.is_some()
    }
}

/// A borrowed bone from a solved frame.
#[derive(Clone, Copy, Debug)]
pub struct SolvedBoneRef<'a> {
    id: BoneId,
    local: &'a crate::pose::BonePose,
    world: WorldTransform,
}

impl SolvedBoneRef<'_> {
    /// Returns the asset-scoped bone ID.
    #[must_use]
    pub const fn id(self) -> BoneId {
        self.id
    }

    /// Returns the final local transform after animation, procedural edits,
    /// and IK.
    #[must_use]
    pub const fn local_transform(self) -> BoneTransform {
        self.local.local_transform
    }

    /// Returns the final skeleton-space transform.
    #[must_use]
    pub const fn world_transform(self) -> WorldTransform {
        self.world
    }
}

/// Renderer-ready, constraint-solved output for one skeleton frame.
#[derive(Debug)]
#[must_use = "inspect or render the solved frame before it is dropped"]
pub struct SolvedFrame<'a> {
    pub(crate) skeleton: &'a Skeleton,
    report: UpdateReport,
}

impl SolvedFrame<'_> {
    /// Returns the immutable asset that defines this frame.
    #[must_use]
    pub fn asset(&self) -> &crate::SkeletonAsset {
        self.skeleton.asset()
    }

    /// Returns lifecycle facts from the player update that produced this
    /// frame.
    #[must_use]
    pub const fn report(&self) -> UpdateReport {
        self.report
    }

    /// Borrows one solved bone after validating its asset identity.
    pub fn bone(&self, bone: BoneId) -> Result<SolvedBoneRef<'_>, IdError> {
        let index = self.skeleton.asset().bone_index(bone)?;
        Ok(SolvedBoneRef {
            id: bone,
            local: &self.skeleton.applied_bones[index],
            world: self.skeleton.world_transforms[index],
        })
    }

    /// Iterates solved bones in source order.
    pub fn bones(
        &self,
    ) -> impl DoubleEndedIterator<Item = SolvedBoneRef<'_>> + ExactSizeIterator + '_ {
        self.skeleton
            .applied_bones
            .iter()
            .enumerate()
            .zip(self.skeleton.world_transforms.iter().copied())
            .map(|((index, local), world)| SolvedBoneRef {
                id: BoneId::new(self.skeleton.asset().key(), index as u32),
                local,
                world,
            })
    }

    /// Borrows one evaluated setup-order slot pose.
    pub fn slot(&self, slot: crate::SlotId) -> Result<SlotPoseRef<'_>, IdError> {
        self.skeleton.slot_pose(slot)
    }

    /// Iterates evaluated slots in back-to-front draw order.
    pub fn slots(
        &self,
    ) -> impl DoubleEndedIterator<Item = SlotPoseRef<'_>> + ExactSizeIterator + '_ {
        self.skeleton.draw_order()
    }

    /// Iterates visible supported attachments in back-to-front draw order.
    ///
    /// Unsupported and non-rendered attachment kinds remain available through
    /// diagnostics, but do not produce misleading geometry.
    pub fn draw_items(&self) -> impl Iterator<Item = DrawItemRef<'_>> + '_ {
        let asset = self.skeleton.asset();
        self.skeleton.draw_order().filter_map(move |slot_pose| {
            let attachment = asset
                .attachment(slot_pose.attachment()?)
                .expect("a runtime attachment index belongs to its immutable asset")
                .as_region()?;
            let slot = asset
                .slot(slot_pose.id())
                .expect("a runtime slot ID belongs to its immutable asset");
            let bone = asset
                .bone(slot.bone())
                .expect("a linked slot bone belongs to its immutable asset");
            let region = RegionDrawItemRef::from_asset(
                asset,
                slot,
                attachment,
                self.skeleton.world_transforms[bone.ordinal()],
                slot_pose.color(),
            )
            .expect("linked draw references belong to one immutable asset");
            Some(DrawItemRef::from(region))
        })
    }

    /// Returns the result of evaluating one IK constraint.
    pub fn ik_status(&self, constraint: IkConstraintId) -> Result<IkSolveStatus, IdError> {
        let index = self.skeleton.asset().ik_constraint_index(constraint)?;
        Ok(self.skeleton.ik_solve_statuses[index])
    }

    /// Iterates IK solve results in authored evaluation order.
    pub fn ik_statuses(
        &self,
    ) -> impl DoubleEndedIterator<Item = (IkConstraintId, IkSolveStatus)> + ExactSizeIterator + '_
    {
        self.skeleton
            .asset()
            .ik_constraints()
            .zip(self.skeleton.ik_solve_statuses.iter().copied())
            .map(|(constraint, status)| (constraint.id(), status))
    }

    /// Returns the result of evaluating one transform constraint.
    pub fn transform_status(
        &self,
        constraint: TransformConstraintId,
    ) -> Result<TransformConstraintSolveStatus, IdError> {
        let index = self
            .skeleton
            .asset()
            .transform_constraint_index(constraint)?;
        Ok(self.skeleton.transform_solve_statuses[index])
    }

    /// Iterates transform-constraint solve results in authored evaluation
    /// order.
    pub fn transform_statuses(
        &self,
    ) -> impl DoubleEndedIterator<Item = (TransformConstraintId, TransformConstraintSolveStatus)>
    + ExactSizeIterator
    + '_ {
        self.skeleton
            .asset()
            .transform_constraints()
            .zip(self.skeleton.transform_solve_statuses.iter().copied())
            .map(|(constraint, status)| (constraint.id(), status))
    }

    /// Iterates retained asset diagnostics that affect this evaluated frame.
    ///
    /// For example, an unsupported attachment is active only while that
    /// attachment is selected by a visible slot. Asset-wide fallbacks are
    /// always active. Unsupported constraint types are conservatively active
    /// because their unsupported semantics cannot be evaluated safely.
    /// Event-scoped diagnostics are delivered by
    /// [`crate::AnimationEvent::diagnostics`] at the instant of emission.
    /// Runtime IK safety fallbacks are exposed separately by
    /// [`Self::ik_statuses`].
    pub fn active_diagnostics(&self) -> impl Iterator<Item = &Diagnostic> + '_ {
        self.skeleton
            .asset()
            .diagnostics()
            .iter()
            .filter(|diagnostic| self.diagnostic_is_active(diagnostic.scope()))
    }

    /// Returns whether an active asset fallback or runtime IK safety fallback
    /// changed this frame.
    #[must_use]
    pub fn has_degradations(&self) -> bool {
        self.active_diagnostics().any(Diagnostic::is_degraded)
            || self
                .skeleton
                .ik_solve_statuses
                .iter()
                .copied()
                .any(IkSolveStatus::is_degraded)
            || self
                .skeleton
                .transform_solve_statuses
                .iter()
                .copied()
                .any(TransformConstraintSolveStatus::is_degraded)
    }

    /// Returns whether a runtime IK safety fallback changed this frame.
    #[must_use]
    pub fn has_runtime_degradations(&self) -> bool {
        self.skeleton
            .ik_solve_statuses
            .iter()
            .copied()
            .any(IkSolveStatus::is_degraded)
            || self
                .skeleton
                .transform_solve_statuses
                .iter()
                .copied()
                .any(TransformConstraintSolveStatus::is_degraded)
    }

    fn diagnostic_is_active(&self, scope: DiagnosticScope) -> bool {
        match scope {
            DiagnosticScope::Asset | DiagnosticScope::Bone(_) => true,
            DiagnosticScope::Slot(slot) => self.slot_is_visible(slot),
            DiagnosticScope::Skin(skin) => self.skin_is_active(skin),
            DiagnosticScope::Animation(animation) => self
                .skeleton
                .asset()
                .animation_index(animation)
                .is_ok_and(|index| {
                    self.skeleton
                        .pose
                        .active_animations
                        .contains(&(index as u32))
                }),
            DiagnosticScope::Event(_event) => false,
            DiagnosticScope::Attachment(attachment) => self.attachment_is_visible(attachment),
            DiagnosticScope::IkConstraint(constraint) => self.ik_is_active(constraint),
            DiagnosticScope::Constraint(constraint) => self.constraint_is_active(constraint),
            DiagnosticScope::AtlasPage(page) => self.atlas_page_is_visible(page),
            DiagnosticScope::AtlasRegion(region) => self.atlas_region_is_visible(region),
        }
    }

    fn slot_is_visible(&self, slot: SlotId) -> bool {
        self.skeleton
            .slot_pose(slot)
            .is_ok_and(|pose| pose.attachment().is_some() && pose.color().alpha() > 0.0)
    }

    fn attachment_is_visible(&self, attachment: AttachmentId) -> bool {
        self.skeleton
            .draw_order()
            .any(|slot| slot.color().alpha() > 0.0 && slot.attachment() == Some(attachment))
    }

    fn skin_is_active(&self, skin: SkinId) -> bool {
        self.skeleton.asset().skin_index(skin).is_ok_and(|index| {
            self.skeleton.skin_layers.contains(&(index as u32))
                || self
                    .skeleton
                    .asset()
                    .default_skin()
                    .is_some_and(|default| default.id() == skin)
        })
    }

    fn ik_is_active(&self, constraint: IkConstraintId) -> bool {
        self.skeleton
            .asset()
            .ik_constraint_index(constraint)
            .is_ok_and(|index| self.skeleton.pose.ik_constraints[index].mix != Mix::ZERO)
    }

    fn constraint_is_active(&self, constraint: ConstraintId) -> bool {
        self.skeleton
            .asset()
            .constraint(constraint)
            .is_ok_and(|constraint| {
                if let Some(constraint) = constraint.as_ik() {
                    self.ik_is_active(constraint.id())
                } else if let Some(constraint) = constraint.as_transform() {
                    self.skeleton
                        .asset()
                        .transform_constraint_index(constraint.id())
                        .is_ok_and(|index| {
                            self.skeleton.pose.transform_constraints[index].any_nonzero()
                        })
                } else {
                    // Assuming inactivity for an unsupported type would
                    // silently hide a potentially applied feature from the
                    // adapter's diagnostic gizmo.
                    true
                }
            })
    }

    fn atlas_page_is_visible(&self, page: AtlasPageId) -> bool {
        self.skeleton.draw_order().any(|slot| {
            slot.color().alpha() > 0.0
                && slot.attachment().is_some_and(|attachment| {
                    self.visible_region(attachment)
                        .is_some_and(|region| region.page() == page)
                })
        })
    }

    fn atlas_region_is_visible(&self, region: AtlasRegionId) -> bool {
        self.skeleton.draw_order().any(|slot| {
            slot.color().alpha() > 0.0
                && slot.attachment().is_some_and(|attachment| {
                    self.skeleton
                        .asset()
                        .attachment(attachment)
                        .ok()
                        .and_then(|attachment| attachment.as_region())
                        .is_some_and(|attachment| attachment.atlas_region() == region)
                })
        })
    }

    fn visible_region(&self, attachment: AttachmentId) -> Option<crate::AtlasRegionRef<'_>> {
        let attachment = self
            .skeleton
            .asset()
            .attachment(attachment)
            .ok()?
            .as_region()?;
        self.skeleton
            .asset()
            .atlas_region(attachment.atlas_region())
            .ok()
    }
}

fn solve_world_and_constraints(skeleton: &mut Skeleton) {
    skeleton.applied_bones.copy_from_slice(&skeleton.pose.bones);
    skeleton.ik_solve_statuses.fill(IkSolveStatus::INACTIVE);
    skeleton
        .transform_solve_statuses
        .fill(TransformConstraintSolveStatus::INACTIVE);
    recompute_world_transforms(skeleton);

    for order_index in 0..skeleton.asset().constraint_evaluation_order().len() {
        let constraint_index = skeleton.asset().constraint_evaluation_order()[order_index] as usize;
        let (ik_constraint, transform_constraint) = {
            let constraint = skeleton.asset().constraint_data(constraint_index);
            (constraint.ik_constraint, constraint.transform_constraint)
        };
        if let Some(index) = ik_constraint {
            solve_ik_constraint(skeleton, index as usize);
        } else if let Some(index) = transform_constraint {
            solve_transform_constraint(skeleton, index as usize);
        }
    }
}

fn solve_ik_constraint(skeleton: &mut Skeleton, constraint_index: usize) {
    let constraint_pose = skeleton.pose.ik_constraints[constraint_index];
    if constraint_pose.mix == Mix::ZERO {
        return;
    }

    let constraint = skeleton.asset().ik_constraint_data(constraint_index);
    let target_world = skeleton.world_transforms[constraint.target as usize].translation();
    let mix = constraint_pose.mix.get();
    let status = match constraint.bones.as_ref() {
        [bone] => apply_one_bone_ik(skeleton, *bone as usize, target_world, mix),
        [parent, child] => apply_two_bone_ik(
            skeleton,
            *parent as usize,
            *child as usize,
            target_world,
            constraint_pose.bend_direction,
            mix,
        ),
        _unsupported => IkSolveStatus::skipped(IkSolveIssue::SingularOrUnderdetermined),
    };
    skeleton.ik_solve_statuses[constraint_index] = status;
    recompute_world_transforms(skeleton);
}

fn solve_transform_constraint(skeleton: &mut Skeleton, constraint_index: usize) {
    let pose = skeleton.pose.transform_constraints[constraint_index];
    let (copies_rotation, supported_mode, source, rotation_offset, bone_count) = {
        let constraint = skeleton.asset().transform_constraint_data(constraint_index);
        (
            constraint.copies_rotation,
            !constraint.local_source
                && !constraint.local_target
                && !constraint.additive
                && !constraint.clamped,
            constraint.source as usize,
            constraint.rotation_offset,
            constraint.bones.len(),
        )
    };
    if !copies_rotation || !supported_mode || pose.mix_rotate == TransformMix::ZERO {
        return;
    }

    let Some(source_rotation) = world_rotation(skeleton.world_transforms[source]) else {
        skeleton.transform_solve_statuses[constraint_index] =
            TransformConstraintSolveStatus::skipped(TransformSolveIssue::SingularOrUnderdetermined);
        return;
    };
    let desired_radians =
        f64::from(source_rotation.as_radians()) + f64::from(rotation_offset.as_radians());
    let desired_world =
        Angle::from_radians(desired_radians.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32)
            .expect("finite source rotation and loaded offset produce a finite saturated angle");
    let mut issue = None;
    for bone_position in 0..bone_count {
        let bone = skeleton
            .asset()
            .transform_constraint_data(constraint_index)
            .bones[bone_position] as usize;
        let Some(current_world) = world_rotation(skeleton.world_transforms[bone]) else {
            issue = Some(TransformSolveIssue::SingularOrUnderdetermined);
            continue;
        };
        let mixed_world = mixed_angle(current_world, desired_world, pose.mix_rotate.get());
        let local = skeleton.applied_bones[bone].local_transform;
        let parent = skeleton
            .asset()
            .bone_data(bone)
            .parent
            .map(|parent| skeleton.world_transforms[parent as usize]);
        let Some(local_rotation) = solve_world_rotation(parent, local, mixed_world) else {
            issue = Some(TransformSolveIssue::SingularOrUnderdetermined);
            continue;
        };
        skeleton.applied_bones[bone].local_transform = replace_rotation(local, local_rotation);
        recompute_world_transforms(skeleton);
    }
    skeleton.transform_solve_statuses[constraint_index] = issue.map_or(
        TransformConstraintSolveStatus::APPLIED,
        TransformConstraintSolveStatus::skipped,
    );
}

fn apply_one_bone_ik(
    skeleton: &mut Skeleton,
    bone: usize,
    target_world: glam::Vec2,
    mix: f32,
) -> IkSolveStatus {
    let local = skeleton.applied_bones[bone].local_transform;
    let parent_world = skeleton
        .asset()
        .bone_data(bone)
        .parent
        .map(|parent| skeleton.world_transforms[parent as usize]);
    let Some(solution) = solve_one_bone_ik(parent_world, local, target_world) else {
        return IkSolveStatus::skipped(IkSolveIssue::SingularOrUnderdetermined);
    };
    let OneBoneIkSolution::Rotation(desired) = solution else {
        return IkSolveStatus::preserved();
    };

    let rotation = mixed_angle(local.rotation(), desired, mix);
    skeleton.applied_bones[bone].local_transform = replace_rotation(local, rotation);
    IkSolveStatus::applied(None, false)
}

fn apply_two_bone_ik(
    skeleton: &mut Skeleton,
    parent: usize,
    child: usize,
    target_world: glam::Vec2,
    bend_direction: BendDirection,
    mix: f32,
) -> IkSolveStatus {
    let parent_local = skeleton.applied_bones[parent].local_transform;
    let parent_for_ik = BoneTransform::new(
        parent_local.translation(),
        parent_local.rotation(),
        parent_local.scale(),
        Shear::ZERO,
    )
    .expect("zeroing finite local shear keeps a finite transform");
    let child_local = skeleton.applied_bones[child].local_transform;
    let grandparent_world = skeleton
        .asset()
        .bone_data(parent)
        .parent
        .map(|grandparent| skeleton.world_transforms[grandparent as usize]);
    let child_length = skeleton.asset().bone_data(child).length;
    let Some(solution) = solve_two_bone_ik(
        grandparent_world,
        parent_for_ik,
        child_local,
        child_length,
        target_world,
        bend_direction,
    ) else {
        return IkSolveStatus::skipped(IkSolveIssue::SingularOrUnderdetermined);
    };

    let parent_rotation = mixed_angle(parent_local.rotation(), solution.parent_rotation, mix);
    let child_rotation = mixed_angle(child_local.rotation(), solution.child_rotation, mix);
    skeleton.applied_bones[parent].local_transform = BoneTransform::new(
        parent_local.translation(),
        parent_rotation,
        parent_local.scale(),
        Shear::ZERO,
    )
    .expect("two-bone IK combines finite transforms and finite solver output");
    skeleton.applied_bones[child].local_transform = BoneTransform::new(
        glam::Vec2::new(
            child_local.translation().x,
            if solution.child_y_was_zeroed {
                solution.child_translation_y
            } else {
                child_local.translation().y
            },
        ),
        child_rotation,
        child_local.scale(),
        child_local.shear(),
    )
    .expect("IK combines finite loaded transforms and finite solver output");

    let target_reach = match solution.reach {
        IkReach::Reached => IkTargetReach::Reachable,
        IkReach::Closest => IkTargetReach::BeyondReach,
    };
    IkSolveStatus::applied(Some(target_reach), solution.child_y_was_zeroed)
}

fn recompute_world_transforms(skeleton: &mut Skeleton) {
    for bone in 0..skeleton.applied_bones.len() {
        let parent = skeleton
            .asset()
            .bone_data(bone)
            .parent
            .map(|parent| skeleton.world_transforms[parent as usize]);
        skeleton.world_transforms[bone] =
            normal_local_to_world(parent, skeleton.applied_bones[bone].local_transform);
    }
}

fn mixed_angle(current: Angle, desired: Angle, mix: f32) -> Angle {
    let radians = f64::from(current.as_radians())
        + f64::from(shortest_angle_delta(current, desired)) * f64::from(mix);
    Angle::from_radians(radians.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32)
        .expect("finite constraint angles and mix produce a finite saturated angle")
}

fn world_rotation(transform: WorldTransform) -> Option<Angle> {
    let axis = transform.x_axis();
    (axis.length_squared() > f32::EPSILON)
        .then(|| Angle::from_radians(axis.y.atan2(axis.x)).ok())
        .flatten()
}

fn replace_rotation(transform: BoneTransform, rotation: Angle) -> BoneTransform {
    BoneTransform::new(
        transform.translation(),
        rotation,
        transform.scale(),
        transform.shear(),
    )
    .expect("replacing one finite component keeps a finite transform")
}

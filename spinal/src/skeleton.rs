use std::{
    mem,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use glam::Vec2;

use crate::{
    Angle, AnimationId, AttachmentId, BendDirection, BoneId, BoneTransform, IdError,
    IkConstraintId, Mix, Rgba, Shear, SkeletonAsset, SkinId, SlotId,
    animation::{
        PlaybackMode, TimelineData, resolve_sample_time, sample_attachment, sample_colour,
        sample_draw_order, sample_ik, sample_scalar, sample_vec2,
    },
    frame::IkSolveStatus,
    pose::{AngleBranches, BlendSwitches, BonePose, IkConstraintPose, PoseBuffers, SlotPose},
    world::WorldTransform,
};

static NEXT_INSTANCE_KEY: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SkeletonInstanceKey(u64);

/// An owned mutable runtime instance of one immutable skeleton asset.
///
/// Construction allocates every fixed-size pose and reconstruction buffer.
/// Absolute sampling, setup restoration, and attachment-only skin changes
/// reuse that storage.
#[derive(Debug)]
pub struct Skeleton {
    instance_key: SkeletonInstanceKey,
    asset: Arc<SkeletonAsset>,
    pub(crate) pose: PoseBuffers,
    pub(crate) applied_bones: Box<[BonePose]>,
    pub(crate) world_transforms: Box<[WorldTransform]>,
    pub(crate) ik_solve_statuses: Box<[IkSolveStatus]>,
    draw_order_scratch: Box<[u32]>,
    pub(crate) skin_layers: Vec<u32>,
    skin_layer_scratch: Vec<u32>,
    skin_revision: u64,
}

impl Skeleton {
    /// Creates an instance in setup pose.
    #[must_use]
    pub fn new(asset: Arc<SkeletonAsset>) -> Self {
        let instance_key = SkeletonInstanceKey(NEXT_INSTANCE_KEY.fetch_add(1, Ordering::Relaxed));
        let pose = PoseBuffers::new(&asset);
        let applied_bones = pose.bones.clone();
        let world_transforms =
            vec![WorldTransform::IDENTITY; asset.bones().len()].into_boxed_slice();
        let ik_solve_statuses =
            vec![IkSolveStatus::INACTIVE; asset.ik_constraints().len()].into_boxed_slice();
        let draw_order_scratch = vec![u32::MAX; asset.slots().len()].into_boxed_slice();
        let skin_layers = Vec::with_capacity(asset.skin_count());
        let skin_layer_scratch = Vec::with_capacity(asset.skin_count());
        let mut skeleton = Self {
            instance_key,
            asset,
            pose,
            applied_bones,
            world_transforms,
            ik_solve_statuses,
            draw_order_scratch,
            skin_layers,
            skin_layer_scratch,
            skin_revision: 0,
        };
        skeleton.reset_slot_attachments_to_setup_pose();
        skeleton
    }

    /// Returns the immutable asset.
    #[must_use]
    pub fn asset(&self) -> &SkeletonAsset {
        &self.asset
    }

    /// Returns the shared asset handle.
    #[must_use]
    pub fn asset_handle(&self) -> &Arc<SkeletonAsset> {
        &self.asset
    }

    /// Resets bones, slots, IK state, and draw order to setup pose.
    ///
    /// Active skin layers are preserved and are used to resolve setup
    /// attachment placeholders.
    pub fn reset_to_setup_pose(&mut self) {
        for (index, pose) in self.pose.bones.iter_mut().enumerate() {
            pose.local_transform = self.asset.bone_data(index).setup_transform;
        }
        for (index, pose) in self.pose.slots.iter_mut().enumerate() {
            pose.color = Rgba::from_rgba8(self.asset.slot_data(index).colour);
        }
        self.reset_slot_attachments_to_setup_pose();
        for (index, pose) in self.pose.ik_constraints.iter_mut().enumerate() {
            let setup = self.asset.ik_constraint_data(index);
            pose.mix = setup.mix;
            pose.bend_direction = setup.bend_direction;
        }
        self.reset_draw_order();
        self.pose.active_animations.clear();
    }

    /// Borrows one unconstrained local bone pose after validating its asset
    /// identity.
    ///
    /// This is animation plus procedural editing state. Constraint-modified
    /// applied locals are available from [`crate::SolvedFrame`].
    pub fn bone_pose(&self, id: BoneId) -> Result<BonePoseRef<'_>, IdError> {
        let index = self.asset.bone_index(id)?;
        Ok(BonePoseRef {
            id,
            pose: &self.pose.bones[index],
        })
    }

    /// Iterates unconstrained local bone poses in source order.
    pub fn bone_poses(
        &self,
    ) -> impl DoubleEndedIterator<Item = BonePoseRef<'_>> + ExactSizeIterator + '_ {
        self.pose
            .bones
            .iter()
            .enumerate()
            .map(|(index, pose)| BonePoseRef {
                id: BoneId::new(self.asset.key(), index as u32),
                pose,
            })
    }

    /// Borrows one setup-order slot pose after validating its asset identity.
    pub fn slot_pose(&self, id: SlotId) -> Result<SlotPoseRef<'_>, IdError> {
        let index = self.asset.slot_index(id)?;
        Ok(SlotPoseRef {
            id,
            pose: &self.pose.slots[index],
        })
    }

    /// Iterates slot poses in the current back-to-front draw order.
    pub fn draw_order(
        &self,
    ) -> impl DoubleEndedIterator<Item = SlotPoseRef<'_>> + ExactSizeIterator + '_ {
        self.pose.draw_order.iter().map(|index| {
            let index = *index as usize;
            SlotPoseRef {
                id: SlotId::new(self.asset.key(), index as u32),
                pose: &self.pose.slots[index],
            }
        })
    }

    /// Borrows one current IK constraint pose.
    pub fn ik_constraint_pose(
        &self,
        id: IkConstraintId,
    ) -> Result<IkConstraintPoseRef<'_>, IdError> {
        let index = self.asset.ik_constraint_index(id)?;
        Ok(IkConstraintPoseRef {
            id,
            pose: &self.pose.ik_constraints[index],
        })
    }

    /// Iterates selected attachment-only skin layers from low to high priority.
    pub fn skin_layers(&self) -> impl DoubleEndedIterator<Item = SkinId> + ExactSizeIterator + '_ {
        self.skin_layers
            .iter()
            .copied()
            .map(|index| SkinId::new(self.asset.key(), index))
    }

    /// Replaces the ordered attachment-only skin layers transactionally.
    ///
    /// Later layers win for the same slot and placeholder, including when a
    /// layer ID is repeated. Missing entries continue through lower layers and
    /// then the default skin. The change immediately restores every slot's
    /// setup attachment through the new composition; colours and draw order
    /// are unchanged.
    pub fn set_skin_layers(&mut self, layers: &[SkinId]) -> Result<(), IdError> {
        self.skin_layer_scratch.clear();
        for id in layers {
            let _index = self.asset.skin_index(*id)?;
        }
        for id in layers.iter().rev() {
            let index = self
                .asset
                .skin_index(*id)
                .expect("all layer IDs were validated above") as u32;
            if !self.skin_layer_scratch.contains(&index) {
                self.skin_layer_scratch.push(index);
            }
        }
        self.skin_layer_scratch.reverse();
        mem::swap(&mut self.skin_layers, &mut self.skin_layer_scratch);
        self.skin_layer_scratch.clear();
        self.reset_slot_attachments_to_setup_pose();
        self.skin_revision = self.skin_revision.wrapping_add(1);
        Ok(())
    }

    /// Resolves a placeholder through active layers and the default skin.
    pub fn resolve_attachment(
        &self,
        slot: SlotId,
        placeholder_name: &str,
    ) -> Result<Option<AttachmentId>, IdError> {
        let slot = self.asset.slot_index(slot)? as u32;
        Ok(self
            .asset
            .resolve_attachment_index(&self.skin_layers, slot, placeholder_name)
            .map(|index| AttachmentId::new(self.asset.key(), index)))
    }

    /// Restores only slot attachments from setup placeholders.
    ///
    /// Slot colours and draw order are left unchanged.
    pub fn reset_slot_attachments_to_setup_pose(&mut self) {
        for (index, pose) in self.pose.slots.iter_mut().enumerate() {
            let placeholder = self.asset.slot_data(index).setup_attachment_name.as_deref();
            pose.attachment_placeholder = placeholder.and_then(|placeholder| {
                self.asset
                    .attachment_placeholder_index(index as u32, placeholder)
            });
            pose.attachment = placeholder.and_then(|placeholder| {
                self.asset
                    .resolve_attachment_index(&self.skin_layers, index as u32, placeholder)
            });
        }
    }

    /// Samples one animation at an absolute position without emitting events.
    ///
    /// Every supported property is restored to setup pose before sampling, so
    /// the result depends only on the asset, skin layers, animation, position,
    /// and playback mode.
    pub fn sample_animation(
        &mut self,
        animation: AnimationId,
        position: Duration,
        playback: PlaybackMode,
    ) -> Result<(), IdError> {
        let animation_index = self.asset.animation_index(animation)?;
        self.reset_to_setup_pose();
        self.pose.active_animations.push(animation_index as u32);
        let animation = self.asset.animation_data(animation_index);
        let time = resolve_sample_time(position, animation.duration, playback);

        for timeline in &animation.timelines {
            match timeline {
                TimelineData::BoneRotate { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let index = *bone as usize;
                        let setup = self.asset.bone_data(index).setup_transform;
                        let current = self.pose.bones[index].local_transform;
                        let rotation = saturated_angle(
                            f64::from(setup.rotation().as_radians())
                                + f64::from(value).to_radians(),
                        );
                        self.pose.bones[index].local_transform = runtime_transform(
                            current.translation(),
                            rotation,
                            current.scale(),
                            current.shear(),
                        );
                    }
                }
                TimelineData::BoneTranslate { bone, frames } => {
                    if let Some([x, y]) = sample_vec2(frames, time) {
                        let index = *bone as usize;
                        let setup = self.asset.bone_data(index).setup_transform;
                        let current = self.pose.bones[index].local_transform;
                        self.pose.bones[index].local_transform = runtime_transform(
                            Vec2::new(
                                saturated_f32(f64::from(setup.translation().x) + f64::from(x)),
                                saturated_f32(f64::from(setup.translation().y) + f64::from(y)),
                            ),
                            current.rotation(),
                            current.scale(),
                            current.shear(),
                        );
                    }
                }
                TimelineData::BoneScale { bone, frames } => {
                    if let Some([x, y]) = sample_vec2(frames, time) {
                        let index = *bone as usize;
                        let setup = self.asset.bone_data(index).setup_transform;
                        let current = self.pose.bones[index].local_transform;
                        self.pose.bones[index].local_transform = runtime_transform(
                            current.translation(),
                            current.rotation(),
                            Vec2::new(
                                saturated_f32(f64::from(setup.scale().x) * f64::from(x)),
                                saturated_f32(f64::from(setup.scale().y) * f64::from(y)),
                            ),
                            current.shear(),
                        );
                    }
                }
                TimelineData::BoneShear { bone, frames } => {
                    if let Some([x, y]) = sample_vec2(frames, time) {
                        let index = *bone as usize;
                        let setup = self.asset.bone_data(index).setup_transform;
                        let current = self.pose.bones[index].local_transform;
                        let shear = Shear::new(
                            saturated_angle(
                                f64::from(setup.shear().x().as_radians())
                                    + f64::from(x).to_radians(),
                            ),
                            saturated_angle(
                                f64::from(setup.shear().y().as_radians())
                                    + f64::from(y).to_radians(),
                            ),
                        );
                        self.pose.bones[index].local_transform = runtime_transform(
                            current.translation(),
                            current.rotation(),
                            current.scale(),
                            shear,
                        );
                    }
                }
                TimelineData::SlotAttachment { slot, frames } => {
                    if let Some(placeholder) = sample_attachment(frames, time) {
                        let pose = &mut self.pose.slots[*slot as usize];
                        pose.attachment_placeholder = placeholder.and_then(|placeholder| {
                            self.asset.attachment_placeholder_index(*slot, placeholder)
                        });
                        pose.attachment = placeholder.and_then(|placeholder| {
                            self.asset.resolve_attachment_index(
                                &self.skin_layers,
                                *slot,
                                placeholder,
                            )
                        });
                    }
                }
                TimelineData::SlotColour { slot, frames } => {
                    if let Some(color) = sample_colour(frames, time) {
                        self.pose.slots[*slot as usize].color = color;
                    }
                }
                TimelineData::Ik { constraint, frames } => {
                    if let Some((mix, bend_direction)) = sample_ik(frames, time) {
                        let pose = &mut self.pose.ik_constraints[*constraint as usize];
                        pose.mix = mix;
                        pose.bend_direction = bend_direction;
                    }
                }
                TimelineData::DrawOrder { frames } => {
                    if let Some(offsets) = sample_draw_order(frames, time) {
                        apply_draw_order(
                            &mut self.pose.draw_order,
                            &mut self.draw_order_scratch,
                            self.pose.slots.len(),
                            offsets,
                        );
                    }
                }
                TimelineData::Events { .. } | TimelineData::Unsupported { .. } => {}
            }
        }
        Ok(())
    }

    fn reset_draw_order(&mut self) {
        for (index, slot) in self.pose.draw_order.iter_mut().enumerate() {
            *slot = index as u32;
        }
    }

    pub(crate) const fn instance_key(&self) -> SkeletonInstanceKey {
        self.instance_key
    }

    pub(crate) fn new_pose_buffers(&self) -> PoseBuffers {
        PoseBuffers::new(&self.asset)
    }

    pub(crate) fn copy_pose_into(&self, target: &mut PoseBuffers) {
        target.copy_from(&self.pose);
    }

    pub(crate) fn replace_pose_from(&mut self, source: &PoseBuffers) {
        self.pose.copy_from(source);
    }

    pub(crate) fn blend_pose_from(
        &mut self,
        source: &PoseBuffers,
        amount: f32,
        switches: BlendSwitches,
        branches: &mut AngleBranches,
    ) {
        self.pose.blend_from(source, amount, switches, branches);
    }

    pub(crate) const fn skin_revision(&self) -> u64 {
        self.skin_revision
    }

    pub(crate) fn remap_pose_attachments(&self, pose: &mut PoseBuffers) {
        for (slot_index, slot) in pose.slots.iter_mut().enumerate() {
            let Some(placeholder_index) = slot.attachment_placeholder else {
                slot.attachment = None;
                continue;
            };
            let placeholder = &self
                .asset
                .attachment_data(placeholder_index as usize)
                .placeholder_name;
            slot.attachment = self.asset.resolve_attachment_index(
                &self.skin_layers,
                slot_index as u32,
                placeholder,
            );
        }
    }
}

fn runtime_transform(
    translation: Vec2,
    rotation: Angle,
    scale: Vec2,
    shear: Shear,
) -> BoneTransform {
    BoneTransform::new(translation, rotation, scale, shear)
        .expect("loaded setup transforms and sampled timeline values remain finite")
}

fn saturated_angle(radians: f64) -> Angle {
    Angle::from_radians(saturated_f32(radians))
        .expect("saturating finite angle conversion remains finite")
}

fn saturated_f32(value: f64) -> f32 {
    value.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32
}

fn apply_draw_order(
    draw_order: &mut [u32],
    scratch: &mut [u32],
    slot_count: usize,
    offsets: &[crate::animation::DrawOrderOffset],
) {
    scratch.fill(u32::MAX);
    for offset in offsets {
        let destination = (offset.slot as i64 + i64::from(offset.offset)) as usize;
        scratch[destination] = offset.slot;
    }

    let mut destination = 0;
    for slot in 0..slot_count as u32 {
        if offsets.iter().any(|offset| offset.slot == slot) {
            continue;
        }
        while scratch[destination] != u32::MAX {
            destination += 1;
        }
        scratch[destination] = slot;
    }
    draw_order.copy_from_slice(scratch);
}

/// A borrowed unconstrained runtime bone pose.
#[derive(Clone, Copy, Debug)]
pub struct BonePoseRef<'a> {
    id: BoneId,
    pose: &'a BonePose,
}

impl BonePoseRef<'_> {
    /// Returns the corresponding asset-scoped bone ID.
    #[must_use]
    pub const fn id(self) -> BoneId {
        self.id
    }

    /// Returns the sampled or procedurally edited local transform before
    /// constraints.
    #[must_use]
    pub const fn local_transform(self) -> BoneTransform {
        self.pose.local_transform
    }
}

/// A borrowed runtime slot pose.
#[derive(Clone, Copy, Debug)]
pub struct SlotPoseRef<'a> {
    id: SlotId,
    pose: &'a SlotPose,
}

impl SlotPoseRef<'_> {
    /// Returns the corresponding asset-scoped slot ID.
    #[must_use]
    pub const fn id(self) -> SlotId {
        self.id
    }

    /// Returns the normalized slot modulation colour.
    #[must_use]
    pub const fn color(self) -> Rgba {
        self.pose.color
    }

    /// Returns the concrete currently selected attachment.
    #[must_use]
    pub fn attachment(self) -> Option<AttachmentId> {
        self.pose
            .attachment
            .map(|index| AttachmentId::new(self.id.asset(), index))
    }
}

/// A borrowed runtime IK constraint pose before solving.
#[derive(Clone, Copy, Debug)]
pub struct IkConstraintPoseRef<'a> {
    id: IkConstraintId,
    pose: &'a IkConstraintPose,
}

impl IkConstraintPoseRef<'_> {
    /// Returns the corresponding asset-scoped IK constraint ID.
    #[must_use]
    pub const fn id(self) -> IkConstraintId {
        self.id
    }

    /// Returns the sampled influence.
    #[must_use]
    pub const fn mix(self) -> Mix {
        self.pose.mix
    }

    /// Returns the sampled bend direction.
    #[must_use]
    pub const fn bend_direction(self) -> BendDirection {
        self.pose.bend_direction
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::{Angle, IdErrorKind, Shear};

    #[test]
    fn instances_start_in_setup_pose_and_reuse_their_buffers() {
        let asset = Arc::new(SkeletonAsset::test_fixture("cat"));
        let mut skeleton = Skeleton::new(Arc::clone(&asset));
        let bone_buffer = skeleton.pose.bones.as_ptr();
        let applied_bone_buffer = skeleton.applied_bones.as_ptr();
        let slot_buffer = skeleton.pose.slots.as_ptr();
        let ik_buffer = skeleton.pose.ik_constraints.as_ptr();
        let draw_buffer = skeleton.pose.draw_order.as_ptr();
        let draw_scratch = skeleton.draw_order_scratch.as_ptr();
        let skin_buffer = skeleton.skin_layers.as_ptr();
        let skin_scratch = skeleton.skin_layer_scratch.as_ptr();
        let head = asset.bone_id("cat-head").expect("head exists");

        skeleton.pose.bones[1].local_transform =
            BoneTransform::new(Vec2::new(2.0, 3.0), Angle::ZERO, Vec2::ONE, Shear::ZERO)
                .expect("the transform is finite");
        skeleton.reset_to_setup_pose();
        let blue = asset.skin_id("blue").expect("skin exists");
        skeleton
            .set_skin_layers(&[blue])
            .expect("skin belongs to the asset");
        skeleton
            .set_skin_layers(&[])
            .expect("empty composition is valid");

        assert_eq!(bone_buffer, skeleton.pose.bones.as_ptr());
        assert_eq!(applied_bone_buffer, skeleton.applied_bones.as_ptr());
        assert_eq!(slot_buffer, skeleton.pose.slots.as_ptr());
        assert_eq!(ik_buffer, skeleton.pose.ik_constraints.as_ptr());
        assert_eq!(draw_buffer, skeleton.pose.draw_order.as_ptr());
        assert_eq!(draw_scratch, skeleton.draw_order_scratch.as_ptr());
        assert_eq!(skin_buffer, skeleton.skin_layers.as_ptr());
        assert_eq!(skin_scratch, skeleton.skin_layer_scratch.as_ptr());
        assert_eq!(
            skeleton
                .bone_pose(head)
                .expect("ID belongs to this asset")
                .local_transform(),
            BoneTransform::IDENTITY
        );
    }

    #[test]
    fn instances_reject_ids_from_other_assets() {
        let own_asset = Arc::new(SkeletonAsset::test_fixture("own"));
        let foreign_asset = SkeletonAsset::test_fixture("foreign");
        let foreign_id = foreign_asset.bone_id("foreign-root").expect("root exists");
        let skeleton = Skeleton::new(own_asset);

        let error = skeleton
            .bone_pose(foreign_id)
            .expect_err("foreign IDs must be rejected");
        assert_eq!(error.kind(), IdErrorKind::ForeignAsset);
    }
}

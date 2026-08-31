use std::{
    mem,
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use glam::Vec2;

use crate::{
    Angle, AnimationId, AttachmentId, BendDirection, BoneId, BoneTransform, IdError,
    IkConstraintId, Mix, Rgba, Shear, SkeletonAsset, SkinId, SlotId, TransformConstraintId,
    TransformMix,
    animation::{
        PlaybackMode, TimelineData, resolve_sample_time, sample_attachment, sample_colour,
        sample_deform, sample_draw_order, sample_ik, sample_scalar, sample_transform, sample_vec2,
    },
    asset::{AttachmentDataKind, TransformConstraintPoseData},
    frame::{IkSolveStatus, TransformConstraintSolveStatus},
    mesh::MeshVerticesData,
    pose::{
        AngleBranches, BlendSwitches, BonePose, ContributionPose, IkConstraintPose, PoseBuffers,
        SlotPose, WeightedContribution,
    },
    world::WorldTransform,
};

static NEXT_INSTANCE_KEY: AtomicU64 = AtomicU64::new(1);

fn saturating_mesh_component(value: f64) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32
    }
}

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
    pub(crate) transform_solve_statuses: Box<[TransformConstraintSolveStatus]>,
    pub(crate) mesh_world_positions: Box<[Vec2]>,
    pub(crate) mesh_vertex_ranges: Box<[Range<usize>]>,
    /// Flat, always-zeroed-at-setup deform deltas, one `[f32; 2]` per mesh
    /// vertex (unweighted attachments) or per bone contribution (weighted
    /// attachments, matching `MeshVerticesData::Weighted`'s per-contribution
    /// shape). Indexed by `deform_ranges`.
    pub(crate) deform_values: Box<[f32]>,
    /// Per-attachment-ordinal ranges into `deform_values`; empty for any
    /// non-mesh attachment.
    pub(crate) deform_ranges: Box<[Range<usize>]>,
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
        let transform_solve_statuses =
            vec![TransformConstraintSolveStatus::INACTIVE; asset.transform_constraints().len()]
                .into_boxed_slice();
        let draw_order_scratch = vec![u32::MAX; asset.slots().len()].into_boxed_slice();
        let mut mesh_vertex_ranges = Vec::with_capacity(asset.attachments().len());
        let mut deform_ranges = Vec::with_capacity(asset.attachments().len());
        let mut mesh_vertex_count = 0_usize;
        let mut deform_value_count = 0_usize;
        for attachment_index in 0..asset.attachments().len() {
            let vertex_start = mesh_vertex_count;
            let deform_start = deform_value_count;
            if let AttachmentDataKind::Mesh(mesh) = &asset.attachment_data(attachment_index).kind {
                let geometry = asset.mesh_geometry_data(mesh.geometry as usize);
                mesh_vertex_count = mesh_vertex_count
                    .checked_add(geometry.vertices.len())
                    .expect("a loaded mesh vertex table fits addressable memory");
                let deform_length = match &geometry.vertices {
                    MeshVerticesData::Unweighted(vertices) => vertices.len() * 2,
                    MeshVerticesData::Weighted { influences, .. } => influences.len() * 2,
                };
                deform_value_count = deform_value_count
                    .checked_add(deform_length)
                    .expect("a loaded mesh deform table fits addressable memory");
            }
            mesh_vertex_ranges.push(vertex_start..mesh_vertex_count);
            deform_ranges.push(deform_start..deform_value_count);
        }
        let mesh_world_positions = vec![Vec2::ZERO; mesh_vertex_count].into_boxed_slice();
        let deform_values = vec![0.0_f32; deform_value_count].into_boxed_slice();
        let skin_layers = Vec::with_capacity(asset.skin_count());
        let skin_layer_scratch = Vec::with_capacity(asset.skin_count());
        let mut skeleton = Self {
            instance_key,
            asset,
            pose,
            applied_bones,
            world_transforms,
            ik_solve_statuses,
            transform_solve_statuses,
            mesh_world_positions,
            mesh_vertex_ranges: mesh_vertex_ranges.into_boxed_slice(),
            deform_values,
            deform_ranges: deform_ranges.into_boxed_slice(),
            draw_order_scratch,
            skin_layers,
            skin_layer_scratch,
            skin_revision: 0,
        };
        skeleton.reset_slot_attachments_to_setup_pose();
        skeleton
    }

    pub(crate) fn update_mesh_world_positions(&mut self) {
        for attachment_index in 0..self.asset.attachments().len() {
            let range = self.mesh_vertex_ranges[attachment_index].clone();
            if range.is_empty() {
                continue;
            }
            let attachment = self.asset.attachment_data(attachment_index);
            let AttachmentDataKind::Mesh(mesh) = &attachment.kind else {
                continue;
            };
            let geometry = self.asset.mesh_geometry_data(mesh.geometry as usize);
            let deform = &self.deform_values[self.deform_ranges[attachment_index].clone()];
            let output = &mut self.mesh_world_positions[range];
            match &geometry.vertices {
                MeshVerticesData::Unweighted(vertices) => {
                    let bone = self.asset.slot_data(attachment.slot as usize).bone as usize;
                    let world = self.world_transforms[bone];
                    for ((position, local), delta) in output
                        .iter_mut()
                        .zip(vertices.iter().copied())
                        .zip(deform.chunks_exact(2))
                    {
                        // Deform is applied to the rest vertex before
                        // skinning: for a rigid (unweighted) mesh that means
                        // adding the delta directly to its one local
                        // position before the single bone transform below.
                        let deformed = Vec2::new(
                            saturating_mesh_component(f64::from(local.x) + f64::from(delta[0])),
                            saturating_mesh_component(f64::from(local.y) + f64::from(delta[1])),
                        );
                        *position = world.transform_point(deformed);
                    }
                }
                MeshVerticesData::Weighted {
                    vertices,
                    influences,
                } => {
                    for (position, influence_range) in output.iter_mut().zip(vertices) {
                        let start = influence_range.start as usize;
                        let end = influence_range.end as usize;
                        let mut blended_x = 0.0_f64;
                        let mut blended_y = 0.0_f64;
                        // Deform is indexed per bone contribution, not per
                        // vertex (a vertex influenced by two bones claims
                        // two delta pairs, in contribution order): each
                        // pair offsets that one contribution's own
                        // bone-local bind position before it is transformed
                        // into world space and blended by weight, mirroring
                        // the unweighted case above (delta added before the
                        // transform) rather than offsetting the
                        // already-blended world position.
                        for (influence, delta) in influences[start..end]
                            .iter()
                            .zip(deform[start * 2..end * 2].chunks_exact(2))
                        {
                            let bind_position = Vec2::new(
                                saturating_mesh_component(
                                    f64::from(influence.bind_position.x) + f64::from(delta[0]),
                                ),
                                saturating_mesh_component(
                                    f64::from(influence.bind_position.y) + f64::from(delta[1]),
                                ),
                            );
                            let transformed = self.world_transforms[influence.bone as usize]
                                .transform_point(bind_position);
                            blended_x += f64::from(transformed.x) * f64::from(influence.weight);
                            blended_y += f64::from(transformed.y) * f64::from(influence.weight);
                        }
                        *position = Vec2::new(
                            saturating_mesh_component(blended_x),
                            saturating_mesh_component(blended_y),
                        );
                    }
                }
            }
        }
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

    /// Resets bones, slots, IK state, draw order, and mesh deform to setup
    /// pose.
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
        for (index, pose) in self.pose.transform_constraints.iter_mut().enumerate() {
            *pose = self.asset.transform_constraint_data(index).setup_pose;
        }
        self.reset_draw_order();
        self.pose.active_animations.clear();
        self.deform_values.fill(0.0);
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

    /// Borrows one current transform constraint pose.
    pub fn transform_constraint_pose(
        &self,
        id: TransformConstraintId,
    ) -> Result<TransformConstraintPoseRef<'_>, IdError> {
        let index = self.asset.transform_constraint_index(id)?;
        Ok(TransformConstraintPoseRef {
            id,
            pose: &self.pose.transform_constraints[index],
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
                TimelineData::BoneTranslateX { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let index = *bone as usize;
                        let setup = self.asset.bone_data(index).setup_transform;
                        let current = self.pose.bones[index].local_transform;
                        self.pose.bones[index].local_transform = runtime_transform(
                            Vec2::new(
                                saturated_f32(f64::from(setup.translation().x) + f64::from(value)),
                                current.translation().y,
                            ),
                            current.rotation(),
                            current.scale(),
                            current.shear(),
                        );
                    }
                }
                TimelineData::BoneTranslateY { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let index = *bone as usize;
                        let setup = self.asset.bone_data(index).setup_transform;
                        let current = self.pose.bones[index].local_transform;
                        self.pose.bones[index].local_transform = runtime_transform(
                            Vec2::new(
                                current.translation().x,
                                saturated_f32(f64::from(setup.translation().y) + f64::from(value)),
                            ),
                            current.rotation(),
                            current.scale(),
                            current.shear(),
                        );
                    }
                }
                TimelineData::BoneScaleX { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let index = *bone as usize;
                        let setup = self.asset.bone_data(index).setup_transform;
                        let current = self.pose.bones[index].local_transform;
                        self.pose.bones[index].local_transform = runtime_transform(
                            current.translation(),
                            current.rotation(),
                            Vec2::new(
                                saturated_f32(f64::from(setup.scale().x) * f64::from(value)),
                                current.scale().y,
                            ),
                            current.shear(),
                        );
                    }
                }
                TimelineData::BoneScaleY { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let index = *bone as usize;
                        let setup = self.asset.bone_data(index).setup_transform;
                        let current = self.pose.bones[index].local_transform;
                        self.pose.bones[index].local_transform = runtime_transform(
                            current.translation(),
                            current.rotation(),
                            Vec2::new(
                                current.scale().x,
                                saturated_f32(f64::from(setup.scale().y) * f64::from(value)),
                            ),
                            current.shear(),
                        );
                    }
                }
                TimelineData::BoneShearX { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let index = *bone as usize;
                        let setup = self.asset.bone_data(index).setup_transform;
                        let current = self.pose.bones[index].local_transform;
                        let shear = Shear::new(
                            saturated_angle(
                                f64::from(setup.shear().x().as_radians())
                                    + f64::from(value).to_radians(),
                            ),
                            current.shear().y(),
                        );
                        self.pose.bones[index].local_transform = runtime_transform(
                            current.translation(),
                            current.rotation(),
                            current.scale(),
                            shear,
                        );
                    }
                }
                TimelineData::BoneShearY { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let index = *bone as usize;
                        let setup = self.asset.bone_data(index).setup_transform;
                        let current = self.pose.bones[index].local_transform;
                        let shear = Shear::new(
                            current.shear().x(),
                            saturated_angle(
                                f64::from(setup.shear().y().as_radians())
                                    + f64::from(value).to_radians(),
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
                TimelineData::Transform { constraint, frames } => {
                    if let Some(pose) = sample_transform(frames, time) {
                        self.pose.transform_constraints[*constraint as usize] = pose;
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
                TimelineData::Deform { attachment, frames } => {
                    // A missing key before the first frame leaves the
                    // buffer at whatever `reset_to_setup_pose` zeroed it to,
                    // matching every other timeline's "absent = setup"
                    // convention.
                    let range = self.deform_ranges[*attachment as usize].clone();
                    sample_deform(frames, time, &mut self.deform_values[range]);
                }
                TimelineData::Events { .. } | TimelineData::Unsupported { .. } => {}
            }
        }
        Ok(())
    }

    pub(crate) fn sample_animation_contribution(
        &self,
        animation: AnimationId,
        position: Duration,
        playback: PlaybackMode,
        contribution: &mut ContributionPose,
    ) -> Result<(), IdError> {
        let animation_index = self.asset.animation_index(animation)?;
        contribution.clear();
        contribution.active_animations.push(animation_index as u32);
        let animation = self.asset.animation_data(animation_index);
        let time = resolve_sample_time(position, animation.duration, playback);

        for timeline in &animation.timelines {
            match timeline {
                TimelineData::BoneRotate { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let setup = self.asset.bone_data(*bone as usize).setup_transform;
                        contribution.bones[*bone as usize].rotation =
                            Some(WeightedContribution::full(saturated_angle(
                                f64::from(setup.rotation().as_radians())
                                    + f64::from(value).to_radians(),
                            )));
                    }
                }
                TimelineData::BoneTranslate { bone, frames } => {
                    if let Some([x, y]) = sample_vec2(frames, time) {
                        let setup = self.asset.bone_data(*bone as usize).setup_transform;
                        let bone = &mut contribution.bones[*bone as usize];
                        bone.translation.x = Some(WeightedContribution::full(saturated_f32(
                            f64::from(setup.translation().x) + f64::from(x),
                        )));
                        bone.translation.y = Some(WeightedContribution::full(saturated_f32(
                            f64::from(setup.translation().y) + f64::from(y),
                        )));
                    }
                }
                TimelineData::BoneScale { bone, frames } => {
                    if let Some([x, y]) = sample_vec2(frames, time) {
                        let setup = self.asset.bone_data(*bone as usize).setup_transform;
                        let bone = &mut contribution.bones[*bone as usize];
                        bone.scale_magnitude.x = Some(WeightedContribution::full(
                            saturated_f32(f64::from(setup.scale().x) * f64::from(x)).abs(),
                        ));
                        bone.scale_magnitude.y = Some(WeightedContribution::full(
                            saturated_f32(f64::from(setup.scale().y) * f64::from(y)).abs(),
                        ));
                    }
                }
                TimelineData::BoneShear { bone, frames } => {
                    if let Some([x, y]) = sample_vec2(frames, time) {
                        let setup = self.asset.bone_data(*bone as usize).setup_transform;
                        let bone = &mut contribution.bones[*bone as usize];
                        bone.shear.x = Some(WeightedContribution::full(saturated_angle(
                            f64::from(setup.shear().x().as_radians()) + f64::from(x).to_radians(),
                        )));
                        bone.shear.y = Some(WeightedContribution::full(saturated_angle(
                            f64::from(setup.shear().y().as_radians()) + f64::from(y).to_radians(),
                        )));
                    }
                }
                TimelineData::BoneTranslateX { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let setup = self.asset.bone_data(*bone as usize).setup_transform;
                        contribution.bones[*bone as usize].translation.x =
                            Some(WeightedContribution::full(saturated_f32(
                                f64::from(setup.translation().x) + f64::from(value),
                            )));
                    }
                }
                TimelineData::BoneTranslateY { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let setup = self.asset.bone_data(*bone as usize).setup_transform;
                        contribution.bones[*bone as usize].translation.y =
                            Some(WeightedContribution::full(saturated_f32(
                                f64::from(setup.translation().y) + f64::from(value),
                            )));
                    }
                }
                TimelineData::BoneScaleX { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let setup = self.asset.bone_data(*bone as usize).setup_transform;
                        contribution.bones[*bone as usize].scale_magnitude.x =
                            Some(WeightedContribution::full(
                                saturated_f32(f64::from(setup.scale().x) * f64::from(value)).abs(),
                            ));
                    }
                }
                TimelineData::BoneScaleY { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let setup = self.asset.bone_data(*bone as usize).setup_transform;
                        contribution.bones[*bone as usize].scale_magnitude.y =
                            Some(WeightedContribution::full(
                                saturated_f32(f64::from(setup.scale().y) * f64::from(value)).abs(),
                            ));
                    }
                }
                TimelineData::BoneShearX { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let setup = self.asset.bone_data(*bone as usize).setup_transform;
                        contribution.bones[*bone as usize].shear.x =
                            Some(WeightedContribution::full(saturated_angle(
                                f64::from(setup.shear().x().as_radians())
                                    + f64::from(value).to_radians(),
                            )));
                    }
                }
                TimelineData::BoneShearY { bone, frames } => {
                    if let Some(value) = sample_scalar(frames, time) {
                        let setup = self.asset.bone_data(*bone as usize).setup_transform;
                        contribution.bones[*bone as usize].shear.y =
                            Some(WeightedContribution::full(saturated_angle(
                                f64::from(setup.shear().y().as_radians())
                                    + f64::from(value).to_radians(),
                            )));
                    }
                }
                TimelineData::SlotColour { slot, frames } => {
                    contribution.slots[*slot as usize].color =
                        sample_colour(frames, time).map(WeightedContribution::full);
                }
                TimelineData::Ik { constraint, frames } => {
                    contribution.ik_constraints[*constraint as usize].mix = sample_ik(frames, time)
                        .map(|(mix, _bend_direction)| WeightedContribution::full(mix));
                }
                TimelineData::Transform { constraint, frames } => {
                    if let Some(pose) = sample_transform(frames, time) {
                        let contribution =
                            &mut contribution.transform_constraints[*constraint as usize];
                        contribution.mix_rotate = Some(WeightedContribution::full(pose.mix_rotate));
                        contribution.mix_x = Some(WeightedContribution::full(pose.mix_x));
                        contribution.mix_y = Some(WeightedContribution::full(pose.mix_y));
                        contribution.mix_scale_x =
                            Some(WeightedContribution::full(pose.mix_scale_x));
                        contribution.mix_scale_y =
                            Some(WeightedContribution::full(pose.mix_scale_y));
                        contribution.mix_shear_y =
                            Some(WeightedContribution::full(pose.mix_shear_y));
                    }
                }
                // Deform is not yet a layered mixer property; see
                // `TimelineData::Deform`'s doc comment.
                TimelineData::SlotAttachment { .. }
                | TimelineData::DrawOrder { .. }
                | TimelineData::Deform { .. }
                | TimelineData::Events { .. }
                | TimelineData::Unsupported { .. } => {}
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
        rotation_path: crate::RotationPath,
        branches: &mut AngleBranches,
    ) {
        self.pose
            .blend_from(source, amount, switches, rotation_path, branches);
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

/// A borrowed runtime transform constraint pose before solving.
#[derive(Clone, Copy, Debug)]
pub struct TransformConstraintPoseRef<'a> {
    id: TransformConstraintId,
    pose: &'a TransformConstraintPoseData,
}

impl TransformConstraintPoseRef<'_> {
    /// Returns the corresponding asset-scoped transform constraint ID.
    #[must_use]
    pub const fn id(self) -> TransformConstraintId {
        self.id
    }

    /// Returns the sampled rotation influence.
    #[must_use]
    pub const fn mix_rotate(self) -> TransformMix {
        self.pose.mix_rotate
    }

    /// Returns the sampled X translation influence.
    #[must_use]
    pub const fn mix_x(self) -> TransformMix {
        self.pose.mix_x
    }

    /// Returns the sampled Y translation influence.
    #[must_use]
    pub const fn mix_y(self) -> TransformMix {
        self.pose.mix_y
    }

    /// Returns the sampled X scale influence.
    #[must_use]
    pub const fn mix_scale_x(self) -> TransformMix {
        self.pose.mix_scale_x
    }

    /// Returns the sampled Y scale influence.
    #[must_use]
    pub const fn mix_scale_y(self) -> TransformMix {
        self.pose.mix_scale_y
    }

    /// Returns the sampled Y shear influence.
    #[must_use]
    pub const fn mix_shear_y(self) -> TransformMix {
        self.pose.mix_shear_y
    }
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

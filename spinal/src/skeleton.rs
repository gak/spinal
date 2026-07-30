use std::sync::Arc;

use crate::{BoneId, BoneTransform, IdError, SkeletonAsset};

#[derive(Debug)]
struct BonePose {
    local_transform: BoneTransform,
}

/// An owned mutable runtime instance of one immutable skeleton asset.
///
/// Construction allocates the fixed-size pose buffers. Resetting reuses that
/// storage; Stage 3 will enforce the complete steady-state allocation contract
/// with allocator-counting tests.
#[derive(Debug)]
pub struct Skeleton {
    asset: Arc<SkeletonAsset>,
    bone_poses: Box<[BonePose]>,
}

impl Skeleton {
    /// Creates an instance in setup pose.
    #[must_use]
    pub fn new(asset: Arc<SkeletonAsset>) -> Self {
        let bone_poses = asset
            .bones()
            .map(|bone| BonePose {
                local_transform: bone.setup_transform(),
            })
            .collect();

        Self { asset, bone_poses }
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

    /// Resets all local bone poses without reallocating their storage.
    pub fn reset_to_setup_pose(&mut self) {
        for (index, pose) in self.bone_poses.iter_mut().enumerate() {
            pose.local_transform = self.asset.bone_data(index).setup_transform;
        }
    }

    /// Borrows one local bone pose after validating its asset identity.
    pub fn bone_pose(&self, id: BoneId) -> Result<BonePoseRef<'_>, IdError> {
        let index = self.asset.bone_index(id)?;
        Ok(BonePoseRef {
            id,
            pose: &self.bone_poses[index],
        })
    }

    /// Iterates local bone poses in source order.
    pub fn bone_poses(
        &self,
    ) -> impl DoubleEndedIterator<Item = BonePoseRef<'_>> + ExactSizeIterator + '_ {
        self.bone_poses
            .iter()
            .enumerate()
            .map(|(index, pose)| BonePoseRef {
                id: BoneId::new(self.asset.key(), index as u32),
                pose,
            })
    }
}

/// A borrowed runtime bone pose.
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

    /// Returns the evaluated local transform.
    #[must_use]
    pub const fn local_transform(self) -> BoneTransform {
        self.pose.local_transform
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
        let buffer = skeleton.bone_poses.as_ptr();
        let head = asset.bone_id("cat-head").expect("head exists");

        skeleton.bone_poses[1].local_transform =
            BoneTransform::new(Vec2::new(2.0, 3.0), Angle::ZERO, Vec2::ONE, Shear::ZERO)
                .expect("the transform is finite");
        skeleton.reset_to_setup_pose();

        assert_eq!(buffer, skeleton.bone_poses.as_ptr());
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

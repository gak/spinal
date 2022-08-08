use bevy_math::Vec2;

#[derive(Debug)]
pub struct Transform {
    /// The constraint name. This is unique for the skeleton.
    pub name: String,

    /// The ordinal for the order constraints are applied.
    pub order_index: u32,

    /// If true, the constraint is only applied when the active skin has the constraint.
    pub skin_required: bool,

    /// The index of the bones whose transform will be controlled by the constraint.
    pub bones: Vec<usize>,

    /// The index of the target bone.
    pub target: usize,

    /// if the target's local transform is affected, else the world transform is affected.
    pub local: bool,

    /// True if the target's transform is adjusted relatively, else the transform is set absolutely.
    pub relative: bool,

    /// The rotation to offset from the target bone.
    pub offset_rotation: f32,

    /// The distance to offset from the target bone.
    pub offset_distance: Vec2,

    /// The scale to offset from the target bone.
    pub offset_scale: Vec2,

    /// The shear to offset from the target bone.
    pub offset_shear_y: f32,

    /// A value from 0 to 1 indicating the influence the constraint has on the bones, where 0 means
    /// no affect, 1 means only the constraint, and between is a mix of the normal pose and the
    /// constraint.
    pub rotate_mix: f32,

    pub translate_mix: Vec2,
    pub scale_mix: Vec2,
    pub shear_mix_y: f32,
}

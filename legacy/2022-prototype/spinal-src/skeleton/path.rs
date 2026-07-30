use strum::FromRepr;

#[derive(Debug)]
pub struct Path {
    /// The constraint name. This is unique for the skeleton.
    pub name: String,

    /// The ordinal for the order constraints are applied.
    pub order_index: u32,

    /// If true, the constraint is only applied when the active skin has the constraint.
    pub skin_required: bool,

    /// The index of the bones whose transform will be controlled by the constraint.
    // TODO: Use a custom Vec type to limit entries.
    pub bones: Vec<usize>,

    /// The index of the target slot.
    pub target_slot: usize,

    /// Determines how the path position is calculated.
    pub position_mode: PathPositionMode,

    /// Determines how the spacing between bones is calculated.
    pub spacing_mode: PathSpacingMode,

    /// Determines how the bone rotation is calculated.
    pub rotate_mode: PathRotateMode,

    /// The rotation to offset from the path rotation.
    pub offset_rotation: f32,

    /// The path position.
    pub position: f32,

    /// The spacing between bones.
    pub spacing: f32,

    /// A value from 0 to 1 indicating the influence the constraint has on the bones, where 0 means
    /// no affect, 1 means only the constraint, and between is a mix of the normal pose and the
    /// constraint.
    pub rotate_mix: f32,

    pub translate_mix: f32,
}

#[derive(Debug, FromRepr)]
pub enum PathPositionMode {
    Fixed,
    Percent,
}

#[derive(Debug, FromRepr)]
pub enum PathSpacingMode {
    Length,
    Fixed,
    Percent,
}

#[derive(Debug, FromRepr)]
pub enum PathRotateMode {
    Tangent,
    Chain,
    ChainScale,
}

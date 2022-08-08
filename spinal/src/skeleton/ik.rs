use strum::FromRepr;

#[derive(Debug)]
pub struct Ik {
    /// The constraint name. This is unique for the skeleton.
    pub name: String,

    /// The ordinal for the order constraints are applied.
    pub order: u32,

    /// skin: If true, the constraint is only applied when the active skin has the constraint.
    /// Assume false if omitted.
    pub skin: bool,

    /// A list of 1 or 2 bone names whose rotation will be controlled by the constraint.
    // TODO: Have some sort of BonesVec to limit the number of bones.
    pub bones: Vec<usize>,

    /// The target bone.
    pub target: usize,

    /// mix: A value from 0 to 1 indicating the influence the constraint has on the bones, where 0
    /// means only FK, 1 means only IK, and between is a mix of FK and IK. Assume 1 if omitted.
    pub mix: f32,

    /// softness: A value for two bone IK, the distance from the maximum reach of the bones that
    /// rotation will slow. Assume 0 if omitted.
    pub softness: f32,

    /// bendPositive: If true, the bones will bend in the positive rotation direction. Assume
    /// false if omitted.
    pub bend_positive: bool,

    /// compress: If true, and only a single bone is being constrained, if the target is too
    /// close, the bone is scaled to reach it. Assume false if omitted.
    pub compress: bool,

    /// stretch: If true, and if the target is out of range, the parent bone is scaled to reach it.
    /// If more than one bone is being constrained and the parent bone has local nonuniform scale,
    /// stretch is not applied. Assume false if omitted.
    pub stretch: bool,

    /// uniform: If true, and only a single bone is being constrained, and compress or stretch is
    /// used, the bone is scaled on both the X and Y axes. Assume false if omitted.
    pub uniform: bool,
}

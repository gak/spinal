use spinal::{BoneId, BoneTransform, SkeletonAsset};
use thiserror::Error;

#[derive(Clone, Debug)]
pub(crate) struct ControlBinding {
    pub(crate) name: Box<str>,
    pub(crate) setup: BoneTransform,
}

#[derive(Clone, Debug)]
pub(crate) struct RigBinding {
    pub(crate) controls: [ControlBinding; 4],
    pub(crate) body: ControlBinding,
}

#[derive(Debug, Error, PartialEq)]
pub(crate) enum RigError {
    #[error("expected exactly four two-bone IK legs, found {0}")]
    WrongLegCount(usize),
    #[error("IK constraint `{0}` has no parent control above its target")]
    MissingControl(String),
    #[error("IK constraint `{0}` has no slot attached to its leg chain")]
    MissingLegSlot(String),
    #[error("the four IK legs do not share a body control below root")]
    MissingBodyControl,
    #[error("the discovered control table could not be formed")]
    InvalidControlTable,
}

pub(crate) fn discover(asset: &SkeletonAsset) -> Result<RigBinding, RigError> {
    let mut candidates = Vec::new();
    let mut upper_legs = Vec::new();
    for constraint in asset
        .ik_constraints()
        .filter(|constraint| constraint.bones().len() == 2)
    {
        let bones = constraint.bones().collect::<Vec<_>>();
        let target = asset
            .bone(constraint.target())
            .expect("a loaded IK target belongs to its asset");
        let control_id = target
            .parent()
            .ok_or_else(|| RigError::MissingControl(constraint.name().to_owned()))?;
        let control = asset
            .bone(control_id)
            .expect("a loaded target parent belongs to its asset");
        let draw_order = asset
            .slots()
            .filter(|slot| bones.contains(&slot.bone()))
            .map(|slot| slot.ordinal())
            .max()
            .ok_or_else(|| RigError::MissingLegSlot(constraint.name().to_owned()))?;
        upper_legs.push(bones[0]);
        candidates.push(Candidate {
            binding: ControlBinding {
                name: control.name().into(),
                setup: control.setup_transform(),
            },
            x: control.setup_transform().translation().x,
            draw_order,
        });
    }
    if candidates.len() != 4 {
        return Err(RigError::WrongLegCount(candidates.len()));
    }

    candidates.sort_by(|left, right| left.x.total_cmp(&right.x));
    let mut hind = candidates.drain(..2).collect::<Vec<_>>();
    let mut fore = candidates;
    hind.sort_by_key(|candidate| candidate.draw_order);
    fore.sort_by_key(|candidate| candidate.draw_order);
    let [hind_far, hind_near] = hind
        .try_into()
        .map_err(|_candidates: Vec<Candidate>| RigError::InvalidControlTable)?;
    let [fore_far, fore_near] = fore
        .try_into()
        .map_err(|_candidates: Vec<Candidate>| RigError::InvalidControlTable)?;

    let body_id = lowest_common_ancestor(asset, &upper_legs)
        .filter(|bone| {
            asset
                .bone(*bone)
                .expect("a discovered ancestor belongs to its asset")
                .parent()
                .is_some()
        })
        .ok_or(RigError::MissingBodyControl)?;
    let body = asset
        .bone(body_id)
        .expect("a discovered common ancestor belongs to its asset");

    Ok(RigBinding {
        controls: [
            hind_near.binding,
            fore_near.binding,
            hind_far.binding,
            fore_far.binding,
        ],
        body: ControlBinding {
            name: body.name().into(),
            setup: body.setup_transform(),
        },
    })
}

#[derive(Clone, Debug)]
struct Candidate {
    binding: ControlBinding,
    x: f32,
    draw_order: usize,
}

fn lowest_common_ancestor(asset: &SkeletonAsset, bones: &[BoneId]) -> Option<BoneId> {
    let first = *bones.first()?;
    ancestors(asset, first).into_iter().find(|candidate| {
        bones
            .iter()
            .all(|bone| is_ancestor(asset, *candidate, *bone))
    })
}

fn is_ancestor(asset: &SkeletonAsset, candidate: BoneId, bone: BoneId) -> bool {
    ancestors(asset, bone).contains(&candidate)
}

fn ancestors(asset: &SkeletonAsset, mut bone: BoneId) -> Vec<BoneId> {
    let mut output = Vec::new();
    loop {
        output.push(bone);
        let reference = asset
            .bone(bone)
            .expect("a loaded parent link remains inside its asset");
        let Some(parent) = reference.parent() else {
            return output;
        };
        bone = parent;
    }
}

#[cfg(test)]
pub(crate) const TEST_JSON: &[u8] = br#"{
  "skeleton":{"spine":"4.3.23"},
  "bones":[
    {"name":"root"},{"name":"body","parent":"root"},
    {"name":"hind-far-upper","parent":"body","x":-8,"y":10,"length":16},{"name":"hind-far-lower","parent":"hind-far-upper","x":16,"length":16},
    {"name":"fore-far-upper","parent":"body","x":10,"y":10,"length":16},{"name":"fore-far-lower","parent":"fore-far-upper","x":16,"length":16},
    {"name":"fore-near-upper","parent":"body","x":8,"y":10,"length":16},{"name":"fore-near-lower","parent":"fore-near-upper","x":16,"length":16},
    {"name":"hind-near-upper","parent":"body","x":-10,"y":10,"length":16},{"name":"hind-near-lower","parent":"hind-near-upper","x":16,"length":16},
    {"name":"hind-far-control","parent":"root","x":-8},{"name":"hind-far-target","parent":"hind-far-control","x":20},
    {"name":"fore-far-control","parent":"root","x":10},{"name":"fore-far-target","parent":"fore-far-control","x":20},
    {"name":"fore-near-control","parent":"root","x":8},{"name":"fore-near-target","parent":"fore-near-control","x":20},
    {"name":"hind-near-control","parent":"root","x":-10},{"name":"hind-near-target","parent":"hind-near-control","x":20}
  ],
  "slots":[
    {"name":"hind-far","bone":"hind-far-lower"},
    {"name":"fore-far","bone":"fore-far-lower"},
    {"name":"fore-near","bone":"fore-near-lower"},
    {"name":"hind-near","bone":"hind-near-lower"}
  ],
  "constraints":[
    {"type":"ik","name":"hind-far","target":"hind-far-target","bones":["hind-far-upper","hind-far-lower"]},
    {"type":"ik","name":"fore-far","target":"fore-far-target","bones":["fore-far-upper","fore-far-lower"]},
    {"type":"ik","name":"fore-near","target":"fore-near-target","bones":["fore-near-upper","fore-near-lower"]},
    {"type":"ik","name":"hind-near","target":"hind-near-target","bones":["hind-near-upper","hind-near-lower"]}
  ],
  "skins":[{"name":"default","attachments":{}}],
  "animations":{}
}"#;

#[cfg(test)]
pub(crate) const TEST_ATLAS: &[u8] =
    b"page.png\nsize:1,1\nformat:RGBA8888\nfilter:Linear,Linear\nrepeat:none\npma:false\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_anatomical_and_draw_order_roles_without_control_names() {
        let asset = spinal::load_json(TEST_JSON, TEST_ATLAS)
            .expect("fixture loads")
            .into_asset();
        let binding = discover(&asset).expect("four leg controls are discovered");

        assert_eq!(
            binding
                .controls
                .iter()
                .map(|control| control.name.as_ref())
                .collect::<Vec<_>>(),
            [
                "hind-near-control",
                "fore-near-control",
                "hind-far-control",
                "fore-far-control"
            ]
        );
        assert_eq!(binding.body.name.as_ref(), "body");
    }
}

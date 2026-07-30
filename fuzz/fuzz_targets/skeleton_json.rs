#![no_main]

use libfuzzer_sys::fuzz_target;
use spinal::{
    Angle, AnimationEvent, AnimationId, AnimationPlayer, BoneTransform, Crossfade, DiagnosticScope,
    EventSink, PlayOptions, PlaybackMode, Skeleton, SkeletonAsset, Transition, load_json,
};

const ATLAS: &[u8] = b"page.png\nregion\n\tbounds: 0, 0, 1, 1\n";
const VALID_SEED: &[u8] = include_bytes!("../corpus/skeleton_json/minimal.json");
static VALID_SEED_CHECK: std::sync::Once = std::sync::Once::new();

fuzz_target!(|data: &[u8]| {
    VALID_SEED_CHECK.call_once(|| {
        let asset = load_json(VALID_SEED, ATLAS)
            .expect("the checked-in skeleton JSON fuzz seed must remain valid")
            .into_asset();
        assert_eq!(asset.animations().count(), 3);
        assert_eq!(asset.ik_constraints().count(), 2);
    });
    if let Ok(report) = load_json(data, ATLAS) {
        traverse(report.into_asset());
    }
});

fn traverse(asset: std::sync::Arc<spinal::SkeletonAsset>) {
    for bone in asset.bones() {
        let _bone = asset
            .bone(bone.id())
            .expect("loader emitted a valid bone ID");
        if let Some(parent) = bone.parent() {
            let _parent = asset
                .bone(parent)
                .expect("loader emitted a valid parent bone ID");
        }
    }
    for slot in asset.slots() {
        let _slot = asset
            .slot(slot.id())
            .expect("loader emitted a valid slot ID");
        let _bone = asset
            .bone(slot.bone())
            .expect("loader emitted a valid slot bone ID");
        if let (Some(default_skin), Some(name)) =
            (asset.default_skin(), slot.setup_attachment_name())
            && let Some(attachment) = default_skin
                .attachment(slot.id(), name)
                .expect("loader emitted an asset-local setup slot")
        {
            let _attachment = asset
                .attachment(attachment)
                .expect("loader emitted a valid default setup attachment ID");
        }
    }
    for skin in asset.skins() {
        let _skin = asset
            .skin(skin.id())
            .expect("loader emitted a valid skin ID");
        for attachment in skin.attachments() {
            let _attachment = asset
                .attachment(attachment.id())
                .expect("loader emitted a valid skin attachment ID");
            let found = skin
                .attachment(attachment.slot(), attachment.placeholder_name())
                .expect("loader emitted an asset-local attachment slot");
            assert_eq!(found, Some(attachment.id()));
        }
    }
    for attachment in asset.attachments() {
        let _attachment = asset
            .attachment(attachment.id())
            .expect("loader emitted a valid attachment ID");
        let _slot = asset
            .slot(attachment.slot())
            .expect("loader emitted a valid attachment slot ID");
        let _skin = asset
            .skin(attachment.skin())
            .expect("loader emitted a valid attachment skin ID");
        if let Some(region) = attachment.as_region() {
            let _region = asset
                .atlas_region(region.atlas_region())
                .expect("loader emitted a valid attachment atlas-region ID");
        }
    }
    for animation in asset.animations() {
        let _animation = asset
            .animation(animation.id())
            .expect("loader emitted a valid animation ID");
    }
    for constraint in asset.constraints() {
        let _constraint = asset
            .constraint(constraint.id())
            .expect("loader emitted a valid constraint ID");
        if let Some(ik) = constraint.as_ik() {
            assert_eq!(ik.constraint().id(), constraint.id());
        }
    }
    for ik in asset.ik_constraints() {
        let _ik = asset
            .ik_constraint(ik.id())
            .expect("loader emitted a valid IK ID");
        let _target = asset
            .bone(ik.target())
            .expect("loader emitted a valid IK target ID");
        for bone in ik.bones() {
            let _bone = asset
                .bone(bone)
                .expect("loader emitted a valid constrained bone ID");
        }
        let _constraint = asset
            .constraint(ik.constraint().id())
            .expect("loader emitted a valid IK constraint bridge");
    }
    for event in asset.event_definitions() {
        let _event = asset
            .event_definition(event.id())
            .expect("loader emitted a valid event ID");
    }
    for page in asset.atlas_pages() {
        let _page = asset
            .atlas_page(page.id())
            .expect("loader emitted a valid atlas page ID");
        for region in page.regions() {
            let _region = asset
                .atlas_region(region.id())
                .expect("loader emitted a valid page region ID");
        }
    }
    for region in asset.atlas_regions() {
        let _region = asset
            .atlas_region(region.id())
            .expect("loader emitted a valid atlas region ID");
        let _page = asset
            .atlas_page(region.page())
            .expect("loader emitted a valid region page ID");
    }
    for diagnostic in asset.diagnostics() {
        match diagnostic.scope() {
            DiagnosticScope::Asset => {}
            DiagnosticScope::Bone(id) => {
                let _bone = asset.bone(id).expect("diagnostic bone ID");
            }
            DiagnosticScope::Slot(id) => {
                let _slot = asset.slot(id).expect("diagnostic slot ID");
            }
            DiagnosticScope::Skin(id) => {
                let _skin = asset.skin(id).expect("diagnostic skin ID");
            }
            DiagnosticScope::Animation(id) => {
                let _animation = asset.animation(id).expect("diagnostic animation ID");
            }
            DiagnosticScope::Event(id) => {
                let _event = asset.event_definition(id).expect("diagnostic event ID");
            }
            DiagnosticScope::Attachment(id) => {
                let _attachment = asset.attachment(id).expect("diagnostic attachment ID");
            }
            DiagnosticScope::IkConstraint(id) => {
                let _ik = asset.ik_constraint(id).expect("diagnostic IK ID");
            }
            DiagnosticScope::Constraint(id) => {
                let _constraint = asset.constraint(id).expect("diagnostic constraint ID");
            }
            DiagnosticScope::AtlasPage(id) => {
                let _page = asset.atlas_page(id).expect("diagnostic page ID");
            }
            DiagnosticScope::AtlasRegion(id) => {
                let _region = asset.atlas_region(id).expect("diagnostic region ID");
            }
            _ => {}
        }
    }
    let mut instance = Skeleton::new(std::sync::Arc::clone(&asset));
    for animation in asset.animations() {
        let midpoint = animation
            .duration()
            .checked_div(2)
            .expect("division by a nonzero constant");
        sample_and_traverse(
            &mut instance,
            &asset,
            animation.id(),
            std::time::Duration::ZERO,
            PlaybackMode::Once,
        );
        sample_and_traverse(
            &mut instance,
            &asset,
            animation.id(),
            midpoint,
            PlaybackMode::Once,
        );
        sample_and_traverse(
            &mut instance,
            &asset,
            animation.id(),
            animation.duration(),
            PlaybackMode::Once,
        );
        sample_and_traverse(
            &mut instance,
            &asset,
            animation.id(),
            animation.duration(),
            PlaybackMode::Loop,
        );
    }
    for skin in asset.skins() {
        instance
            .set_skin_layers(&[skin.id()])
            .expect("loader emitted an asset-local skin ID");
        instance.reset_to_setup_pose();
    }

    let mut player = AnimationPlayer::new(&instance);
    for animation in asset.animations() {
        player
            .play(animation.id(), PlayOptions::once())
            .expect("loader emitted an asset-local animation ID");
        let frame = player
            .update(&mut instance, animation.duration(), &mut ())
            .expect("player remains bound to its instance")
            .solve();
        traverse_frame(&frame);
    }
    exercise_live_crossfades(&asset, &mut instance, &mut player);
}

fn exercise_live_crossfades(
    asset: &SkeletonAsset,
    instance: &mut Skeleton,
    player: &mut AnimationPlayer,
) {
    let mut animations = asset.animations().map(|animation| animation.id());
    let (Some(source), Some(target)) = (animations.next(), animations.next()) else {
        return;
    };
    let interrupt = animations.next().unwrap_or(source);
    let override_bone = asset.bones().next().map(|bone| bone.id());
    let crossfade = Transition::Crossfade(Crossfade::new(std::time::Duration::from_millis(4)));
    let mut events = ValidatingEventSink { asset, count: 0 };

    player
        .play(source, PlayOptions::looping())
        .expect("loader emitted an asset-local source animation");
    let frame = player
        .update(instance, std::time::Duration::from_millis(1), &mut events)
        .expect("player remains bound to its instance")
        .solve();
    traverse_frame(&frame);
    drop(frame);

    player
        .play(target, PlayOptions::looping().with_transition(crossfade))
        .expect("loader emitted an asset-local target animation");
    let mut pose = player
        .update(instance, std::time::Duration::from_millis(1), &mut events)
        .expect("player remains bound during a live crossfade");
    if let Some(bone) = override_bone {
        let mut edit = pose.edit();
        let local = edit
            .bone_local(bone)
            .expect("loader emitted an asset-local override bone");
        let rotation = Angle::from_degrees(local.rotation().as_degrees() + 1.0)
            .expect("finite loaded rotations remain finite after a one-degree edit");
        let replacement =
            BoneTransform::new(local.translation(), rotation, local.scale(), local.shear())
                .expect("a finite loaded transform remains valid after rotation");
        edit.set_bone_local(bone, replacement)
            .expect("loader emitted an asset-local override bone");
    }
    let frame = pose.solve();
    traverse_frame(&frame);
    drop(frame);

    player
        .play(interrupt, PlayOptions::looping().with_transition(crossfade))
        .expect("loader emitted an asset-local interrupt animation");
    for delta in [
        std::time::Duration::from_millis(1),
        std::time::Duration::from_millis(4),
    ] {
        let frame = player
            .update(instance, delta, &mut events)
            .expect("player remains bound through rapid interruption")
            .solve();
        traverse_frame(&frame);
    }
    let _event_count = events.count;
}

struct ValidatingEventSink<'a> {
    asset: &'a SkeletonAsset,
    count: usize,
}

impl EventSink for ValidatingEventSink<'_> {
    fn event(&mut self, event: AnimationEvent<'_>) {
        let definition = event.definition();
        let _definition = self
            .asset
            .event_definition(definition.id())
            .expect("player emitted an asset-local event definition");
        let _animation = self
            .asset
            .animation(event.animation())
            .expect("player emitted an asset-local animation");
        let _payload = (
            event.playback(),
            event.loop_index(),
            event.local_time(),
            event.integer(),
            event.float(),
            event.string(),
            event.volume(),
            event.balance(),
            event.has_degradations(),
        );
        self.count = self.count.saturating_add(1);
    }
}

fn sample_and_traverse(
    instance: &mut Skeleton,
    asset: &SkeletonAsset,
    animation: AnimationId,
    position: std::time::Duration,
    playback: PlaybackMode,
) {
    instance
        .sample_animation(animation, position, playback)
        .expect("loader emitted an asset-local animation ID");

    for bone in instance.bone_poses() {
        let _id = bone.id();
        let _local_transform = bone.local_transform();
    }
    for slot in asset.slots() {
        let pose = instance
            .slot_pose(slot.id())
            .expect("loader emitted an asset-local slot ID");
        let _colour = pose.color();
        if let Some(attachment) = pose.attachment() {
            let _attachment = asset
                .attachment(attachment)
                .expect("runtime emitted an asset-local attachment ID");
        }
    }
    for slot in instance.draw_order() {
        let _slot = asset
            .slot(slot.id())
            .expect("runtime emitted an asset-local draw-order slot ID");
    }
    for constraint in asset.ik_constraints() {
        let pose = instance
            .ik_constraint_pose(constraint.id())
            .expect("loader emitted an asset-local IK ID");
        let _mix = pose.mix();
        let _bend_direction = pose.bend_direction();
    }

    let frame = instance.editable_pose().solve();
    traverse_frame(&frame);
}

fn traverse_frame(frame: &spinal::SolvedFrame<'_>) {
    for bone in frame.bones() {
        let _id = bone.id();
        let _local = bone.local_transform();
        let _world = bone.world_transform();
    }
    for item in frame.draw_items() {
        std::hint::black_box(item);
    }
    for (constraint, status) in frame.ik_statuses() {
        std::hint::black_box((constraint, status));
    }
    for diagnostic in frame.active_diagnostics() {
        std::hint::black_box(diagnostic);
    }
    let _has_degradations = frame.has_degradations();
}

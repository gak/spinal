#![no_main]

use libfuzzer_sys::fuzz_target;
use spinal::{DiagnosticScope, Skeleton, load_json};

const ATLAS: &[u8] = b"page.png\nregion\n\tbounds: 0, 0, 1, 1\n";

fuzz_target!(|data: &[u8]| {
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
        if let Some(attachment) = slot.setup_attachment() {
            let _attachment = asset
                .attachment(attachment)
                .expect("loader emitted a valid setup attachment ID");
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
    let _instance = Skeleton::new(asset);
}

use std::{
    collections::BTreeSet,
    f32::consts::{PI, TAU},
    sync::Arc,
    time::Duration,
};

use spinal::{
    AlphaEncoding, AnimationId, AnimationMixer, AttachmentId, AttachmentKind, Crossfade,
    DiagnosticCode, DiagnosticScope, DiagnosticSeverity, DrawItemRef, IkTargetReach, LoadDocument,
    LoadErrorKind, MixCurve, PlayOptions, PlaybackMode, Skeleton, SkeletonAsset, TextureFilter,
    TextureFormat, Transition, WrapMode, glam::Vec2,
};

use crate::{
    report::{AnimationInventory, Inventory, Report},
    source::{PageInspection, SourceFiles},
};

const REQUIRED_ANIMATIONS: &[&str] = &["walk", "jump", "eat", "sit", "sleep", "loaf", "falling"];
const REQUIRED_HATS: &[&str] = &[
    "item/hat_red_beret",
    "item/hat_flower_crown",
    "item/hat_straw_sunhat",
];
const REQUIRED_COLLARS: &[&str] = &["item/collar_red", "item/collar_bell", "item/collar_founder"];
const REQUIRED_GLASSES: &[&str] = &["item/glasses_round", "item/glasses_heart"];
const OPTIONAL_LAYERS: &[&str] = &[
    "breathe",
    "tail-mood",
    "look",
    "blink",
    "ear-twitch",
    "tail-refuse",
];
const CROSSFADE: Duration = Duration::from_millis(150);
const CROSSFADE_STEP: Duration = Duration::from_millis(10);
const SOURCE_PHASES: &[f64] = &[0.0, 0.25, 0.5, 0.75];
const WALK_SPEEDS: &[f32] = &[0.5, 1.0, 1.5, 3.0];
const NORMAL_SPEEDS: &[f32] = &[1.0];

pub(crate) fn loafstead_demo(source: &SourceFiles) -> Report {
    let mut report = Report::new(
        source.json_path().display().to_string(),
        source.atlas_path().display().to_string(),
    );
    let load_report = match spinal::load_json(source.json(), source.atlas()) {
        Ok(load_report) => load_report,
        Err(error) => {
            let location = load_error_scope(&error);
            report.error(
                format!("load-{}", load_error_name(error.kind())),
                location,
                error.to_string(),
                "Correct the Spine JSON/text-atlas export, then rerun this check.",
            );
            report.finish();
            return report;
        }
    };
    let asset = Arc::clone(load_report.asset());
    report.spine_version = Some(asset.spine_version().to_owned());
    report.inventory = Some(inventory(&asset));

    check_version(&asset, &mut report);
    check_diagnostics(&asset, &mut report);
    let pages = match source.inspect_pages(&asset) {
        Ok(pages) => pages,
        Err(error) => {
            report.source_error(error.code(), error.to_string());
            return report;
        }
    };
    check_atlas(&asset, &pages, &mut report);
    check_setup_pose(&asset, &mut report);
    let usable_animations = check_required_animations(&asset, &mut report);
    check_cosmetics(&asset, &mut report);
    check_optional_layers(&asset, &mut report);
    check_crossfades(&asset, &usable_animations, &mut report);
    report.warning(
        "export-preset-unverified",
        "export:preset",
        "The final JSON, atlas, and PNG files cannot prove every editor and texture-packer toggle.",
        "Share and version the Spine export preset and pack.json with production exports.",
    );
    report.finish();
    report
}

fn check_version(asset: &SkeletonAsset, report: &mut Report) {
    if asset.spine_version() != spinal::TARGET_SPINE_VERSION {
        report.error(
            "spine-version-mismatch",
            "skeleton:version",
            format!(
                "Loafstead pins Spine {}, but this export declares {}.",
                spinal::TARGET_SPINE_VERSION,
                asset.spine_version()
            ),
            format!(
                "Open and export the project with Spine {}.",
                spinal::TARGET_SPINE_VERSION
            ),
        );
    }
}

fn check_diagnostics(asset: &SkeletonAsset, report: &mut Report) {
    for diagnostic in asset.diagnostics() {
        if diagnostic.code() == DiagnosticCode::AlphaEncodingMismatch {
            // The page contract below emits the more actionable, page-named finding.
            continue;
        }
        let code = format!("runtime-{}", diagnostic_code_name(diagnostic.code()));
        let scope = diagnostic_scope(asset, diagnostic.scope());
        match diagnostic.severity() {
            DiagnosticSeverity::Warning => report.warning(
                code,
                scope,
                diagnostic.message(),
                "Review the retained metadata and remove it if it was not intentional.",
            ),
            DiagnosticSeverity::Degraded => report.error(
                code,
                scope,
                diagnostic.message(),
                "Remove or replace the unsupported Spine feature before the demo export.",
            ),
            _future => report.error(
                "runtime-future-diagnostic",
                scope,
                diagnostic.message(),
                "Update Spinal's loafstead-demo profile before accepting this export.",
            ),
        }
    }
}

fn check_atlas(asset: &SkeletonAsset, pages: &[PageInspection], report: &mut Report) {
    for (page, image) in asset.atlas_pages().zip(pages) {
        let scope = format!("atlas-page:{}", page.name());
        if page.alpha_encoding() != AlphaEncoding::Straight {
            report.error(
                "atlas-premultiplied-alpha",
                &scope,
                "The page is packed with premultiplied alpha (`pma: true`), while Loafstead expects straight alpha.",
                "Export with Premultiply alpha off and Bleed on.",
            );
        }
        if page.format() != TextureFormat::Rgba8888 {
            report.error(
                "atlas-format",
                &scope,
                format!(
                    "The atlas page format is `{}`; Loafstead requires RGBA8888.",
                    page.format_token()
                ),
                "Set the atlas page format to RGBA8888.",
            );
        }
        if page.min_filter() != TextureFilter::Linear || page.mag_filter() != TextureFilter::Linear
        {
            report.error(
                "atlas-filter",
                &scope,
                format!(
                    "The page filters are `{}`, `{}`; Loafstead requires Linear, Linear.",
                    page.min_filter_token(),
                    page.mag_filter_token()
                ),
                "Set both texture filters to Linear in the shared pack preset.",
            );
        }
        if page.wrap() != WrapMode::CLAMP {
            report.error(
                "atlas-wrap",
                &scope,
                "The page repeats on at least one axis; Loafstead requires clamp/no repeat.",
                "Set texture wrap/repeat to none.",
            );
        }
        if (page.scale() - 1.0).abs() > f32::EPSILON {
            report.error(
                "atlas-scale",
                &scope,
                format!(
                    "The atlas export scale is {}; Loafstead requires 1.",
                    page.scale()
                ),
                "Pack and export at scale 1.",
            );
        }
        if page.size().width() == 0 || page.size().height() == 0 {
            report.error(
                "atlas-page-size-missing",
                &scope,
                "The text atlas does not declare a nonzero page size.",
                "Enable page size metadata in the text-atlas export.",
            );
        }
        check_page_image(image, report);
    }
}

fn check_page_image(image: &PageInspection, report: &mut Report) {
    let scope = format!("atlas-page:{}", image.name);
    if let Some(problem) = &image.problem {
        report.error(
            "atlas-page-unreadable",
            scope,
            format!("{} ({})", problem, image.path.display()),
            "Place a valid PNG at the atlas page path and export the bundle again.",
        );
        return;
    }
    if image.color_type != Some(png::ColorType::Rgba)
        || image.bit_depth != Some(png::BitDepth::Eight)
    {
        report.error(
            "atlas-page-not-rgba8",
            &scope,
            format!(
                "The PNG decodes as {:?}/{:?}; Loafstead requires 8-bit RGBA.",
                image.color_type, image.bit_depth
            ),
            "Export the atlas page as an 8-bit RGBA PNG.",
        );
    }
    let declared = (image.declared_size.width(), image.declared_size.height());
    if image.actual_size != Some(declared) {
        report.error(
            "atlas-page-dimension-mismatch",
            &scope,
            format!(
                "The text atlas declares {}x{}, but the PNG is {:?}.",
                declared.0, declared.1, image.actual_size
            ),
            "Repack the atlas so its text metadata and PNG dimensions match.",
        );
    }
}

fn check_setup_pose(asset: &Arc<SkeletonAsset>, report: &mut Report) {
    let mut skeleton = Skeleton::new(Arc::clone(asset));
    let frame = skeleton.editable_pose().solve();
    let facts = frame_facts(&frame);
    if facts.draw_count == 0 {
        report.error(
            "setup-pose-not-drawable",
            "skeleton:setup-pose",
            "The default setup pose produces no supported region or mesh draw.",
            "Put at least one visible region or mesh attachment in the default skin/setup pose.",
        );
    }
    for problem in facts.geometry_problems {
        report.error(
            "setup-pose-invalid-geometry",
            "skeleton:setup-pose",
            problem,
            "Correct the attachment, mesh, atlas region, or bone transforms and re-export.",
        );
    }
    for problem in facts.transform_problems {
        report.error(
            "setup-pose-transform-constraint",
            "skeleton:setup-pose",
            problem,
            "Correct the transform-constraint geometry before the demo export.",
        );
    }
    for warning in facts.ik_warnings {
        report.warning(
            "setup-pose-ik",
            "skeleton:setup-pose",
            warning,
            "Review the IK target and chain geometry in setup pose.",
        );
    }
}

fn check_required_animations(
    asset: &Arc<SkeletonAsset>,
    report: &mut Report,
) -> Vec<(&'static str, AnimationId)> {
    let mut usable = Vec::new();
    for &name in REQUIRED_ANIMATIONS {
        let Some(id) = asset.animation_id(name) else {
            report.error(
                "required-animation-missing",
                format!("animation:{name}"),
                format!("Required Loafstead animation `{name}` is missing."),
                format!("Add a nonempty animation named exactly `{name}` (lowercase)."),
            );
            continue;
        };
        let animation = asset
            .animation(id)
            .expect("an animation ID resolved by this asset belongs to it");
        if animation.duration().is_zero() {
            report.error(
                "required-animation-empty",
                format!("animation:{name}"),
                format!("Required animation `{name}` has zero duration."),
                "Author at least one nonzero-time pose timeline for this clip.",
            );
            continue;
        }
        if animation.properties().len() == 0 {
            report.error(
                "required-animation-no-pose",
                format!("animation:{name}"),
                format!("Required animation `{name}` contains no supported pose timelines."),
                "Animate at least one supported bone, slot, IK, or transform-constraint property.",
            );
            continue;
        }
        if animation.duration() < CROSSFADE {
            report.warning(
                "animation-shorter-than-crossfade",
                format!("animation:{name}"),
                format!(
                    "`{name}` is {:.3}s, shorter than Loafstead's 0.150s crossfade.",
                    animation.duration().as_secs_f64()
                ),
                "Preview rapid state changes; lengthen the clip if the blend looks frozen or abrupt.",
            );
        }
        let samples = sample_animation(asset, id, name, report);
        if let Some(samples) = samples {
            check_loop_risks(name, &samples, report);
            report.readiness.required_animations += 1;
            usable.push((name, id));
        }
    }
    usable
}

fn sample_animation(
    asset: &Arc<SkeletonAsset>,
    id: AnimationId,
    name: &str,
    report: &mut Report,
) -> Option<[FrameFacts; 3]> {
    let duration = asset
        .animation(id)
        .expect("checked animation ID belongs to the asset")
        .duration();
    let positions = [
        Duration::ZERO,
        Duration::from_secs_f64(duration.as_secs_f64() / 2.0),
        duration,
    ];
    let mut samples = Vec::with_capacity(3);
    for position in positions {
        let mut skeleton = Skeleton::new(Arc::clone(asset));
        if let Err(error) = skeleton.sample_animation(id, position, PlaybackMode::Once) {
            report.error(
                "animation-sample-failed",
                format!("animation:{name}"),
                format!(
                    "Could not sample `{name}` at {:.3}s: {error}",
                    position.as_secs_f64()
                ),
                "Correct the clip's references and timelines, then export again.",
            );
            return None;
        }
        let frame = skeleton.editable_pose().solve();
        samples.push(frame_facts(&frame));
    }

    let mut errors = BTreeSet::new();
    let mut ik_warnings = BTreeSet::new();
    let mut transform_problems = BTreeSet::new();
    for sample in &samples {
        if sample.draw_count == 0 {
            errors.insert("the sampled pose has no drawable region or mesh".to_owned());
        }
        errors.extend(sample.geometry_problems.iter().cloned());
        ik_warnings.extend(sample.ik_warnings.iter().cloned());
        transform_problems.extend(sample.transform_problems.iter().cloned());
    }
    for error in errors {
        report.error(
            "animation-frame-invalid",
            format!("animation:{name}"),
            error,
            "Correct the clip so setup, midpoint, and endpoint all produce finite drawable output.",
        );
    }
    for warning in ik_warnings {
        report.warning(
            "animation-ik-risk",
            format!("animation:{name}"),
            warning,
            "Review the IK target and chain at the clip's setup, midpoint, and endpoint.",
        );
    }
    for problem in transform_problems {
        report.error(
            "animation-transform-constraint",
            format!("animation:{name}"),
            problem,
            "Correct the transform-constraint geometry at this clip's representative poses.",
        );
    }
    if !samples.iter().all(|sample| {
        sample.draw_count > 0
            && sample.geometry_problems.is_empty()
            && sample.transform_problems.is_empty()
    }) {
        return None;
    }
    samples.try_into().ok()
}

fn check_loop_risks(name: &str, samples: &[FrameFacts; 3], report: &mut Report) {
    let [start, midpoint, end] = samples;
    let mut discontinuous = Vec::new();
    let mut scale_flips = Vec::new();
    for ((start_bone, middle_bone), end_bone) in
        start.bones.iter().zip(&midpoint.bones).zip(&end.bones)
    {
        if transform_discontinuous(start_bone, end_bone) {
            discontinuous.push(start_bone.name.clone());
        }
        if scale_sign_changed(start_bone.scale, middle_bone.scale)
            || scale_sign_changed(middle_bone.scale, end_bone.scale)
        {
            scale_flips.push(start_bone.name.clone());
        }
    }
    if !discontinuous.is_empty() {
        report.warning(
            "loop-boundary-discontinuity",
            format!("animation:{name}"),
            format!(
                "The loop endpoint differs visibly from its start on bone(s): {}.",
                concise_names(&discontinuous)
            ),
            "Match the first and last poses, or preview the loop and confirm the authored snap is intentional.",
        );
    }
    if !scale_flips.is_empty() {
        report.warning(
            "animation-scale-sign-flip",
            format!("animation:{name}"),
            format!(
                "Signed scale changes facing on bone(s): {}.",
                concise_names(&scale_flips)
            ),
            "Let Loafstead flip the whole skeleton; avoid internal signed-scale facing changes unless intentional.",
        );
    }
    if let (Some(start_root), Some(mid_root), Some(end_root)) = (
        start.bones.first(),
        midpoint.bones.first(),
        end.bones.first(),
    ) {
        let travel = start_root
            .translation
            .distance(mid_root.translation)
            .max(mid_root.translation.distance(end_root.translation))
            .max(start_root.translation.distance(end_root.translation));
        if travel > 0.5 {
            report.warning(
                "animation-root-motion",
                format!("animation:{name}"),
                format!("The root bone moves by up to {travel:.2} skeleton units."),
                "Keep locomotion in Loafstead's entity movement, or confirm this root motion is intentional.",
            );
        }
    }
}

fn check_cosmetics(asset: &Arc<SkeletonAsset>, report: &mut Report) {
    let hats = check_named_cosmetics(asset, REQUIRED_HATS, "hat", report);
    let collars = check_named_cosmetics(asset, REQUIRED_COLLARS, "collar", report);
    let mut glasses = check_named_cosmetics(asset, REQUIRED_GLASSES, "glasses", report);
    report.readiness.hats = hats.len();
    report.readiness.collars = collars.len();
    report.readiness.glasses = glasses.len();

    let mut extra_glasses = asset
        .skins()
        .filter(|skin| {
            skin.name().starts_with("item/glasses_")
                && !REQUIRED_GLASSES.contains(&skin.name())
                && cosmetic_skin_problem(asset, *skin).is_none()
        })
        .map(|skin| (skin.name().to_owned(), skin.id()))
        .collect::<Vec<_>>();
    extra_glasses.sort_by(|left, right| left.0.cmp(&right.0));
    if let Some((name, id)) = extra_glasses.first() {
        report.readiness.glasses += 1;
        report.readiness.third_glasses_skin = Some(name.clone());
        glasses.push(*id);
        report.warning(
            "third-glasses-mapping-pending",
            format!("skin:{name}"),
            format!("Third glasses skin `{name}` is present, but Loafstead has no stable catalogue mapping for it yet."),
            "Add this exact skin name to Loafstead's cosmetic catalogue and Spine bridge before the demo build.",
        );
    } else {
        report.error(
            "third-glasses-skin-missing",
            "skin:item/glasses_*",
            "The demo promises three glasses, but only the two currently mapped glasses skins are present.",
            "Add one more nonempty attachment-only skin named `item/glasses_<stable-name>`.",
        );
    }
    check_cosmetic_composition(asset, &hats, &collars, &glasses, report);
}

fn check_named_cosmetics(
    asset: &Arc<SkeletonAsset>,
    names: &[&str],
    kind: &str,
    report: &mut Report,
) -> Vec<spinal::SkinId> {
    let mut ready = Vec::new();
    for &name in names {
        let Some(id) = asset.skin_id(name) else {
            report.error(
                "required-cosmetic-skin-missing",
                format!("skin:{name}"),
                format!("Required demo {kind} skin `{name}` is missing."),
                format!("Add a nonempty attachment-only skin named exactly `{name}`."),
            );
            continue;
        };
        let skin = asset
            .skin(id)
            .expect("a skin ID resolved by this asset belongs to it");
        if let Some(problem) = cosmetic_skin_problem(asset, skin) {
            report.error(
                "required-cosmetic-skin-invalid",
                format!("skin:{name}"),
                problem,
                "Keep the skin nonempty and limit it to drawable region or mesh attachments.",
            );
        } else {
            ready.push(id);
        }
    }
    ready
}

fn cosmetic_skin_problem(asset: &Arc<SkeletonAsset>, skin: spinal::SkinRef<'_>) -> Option<String> {
    let attachments = skin.attachments().collect::<Vec<_>>();
    if attachments.is_empty() {
        return Some(format!("Cosmetic skin `{}` is empty.", skin.name()));
    }
    let unsupported = attachments
        .iter()
        .filter(|attachment| {
            !matches!(
                attachment.kind(),
                AttachmentKind::Region | AttachmentKind::Mesh
            )
        })
        .map(|attachment| attachment.name())
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Some(format!(
            "Cosmetic skin `{}` contains non-drawable attachment(s): {}.",
            skin.name(),
            unsupported.join(", ")
        ));
    }
    let own_attachments = attachments
        .iter()
        .map(|attachment| attachment.id())
        .collect::<Vec<_>>();
    let mut skeleton = Skeleton::new(Arc::clone(asset));
    skeleton
        .set_skin_layers(&[skin.id()])
        .expect("a skin from this asset belongs to its skeleton");
    let frame = skeleton.editable_pose().solve();
    let facts = frame_facts(&frame);
    if !facts
        .visible_attachments
        .iter()
        .any(|attachment| own_attachments.contains(attachment))
    {
        return Some(format!(
            "Cosmetic skin `{}` does not select any visible attachment in setup pose.",
            skin.name()
        ));
    }
    None
}

fn check_cosmetic_composition(
    asset: &Arc<SkeletonAsset>,
    hats: &[spinal::SkinId],
    collars: &[spinal::SkinId],
    glasses: &[spinal::SkinId],
    report: &mut Report,
) {
    if hats.len() != REQUIRED_HATS.len()
        || collars.len() != REQUIRED_COLLARS.len()
        || glasses.len() != 3
    {
        return;
    }
    for &hat in hats {
        for &collar in collars {
            for &glasses in glasses {
                let layers = [hat, collar, glasses];
                if let Some((context, missing)) = first_invisible_cosmetic_sample(asset, &layers) {
                    let selected = layers
                        .iter()
                        .map(|skin_id| {
                            asset
                                .skin(*skin_id)
                                .expect("profile skin ID belongs to the asset")
                                .name()
                        })
                        .collect::<Vec<_>>()
                        .join(" + ");
                    report.error(
                        "cosmetic-composition-invisible",
                        "skins:hat+collar+glasses",
                        format!(
                            "Combination `{selected}` does not preserve required visible attachment(s) {} in {context}.",
                            missing.join(", ")
                        ),
                        "Give hats, collars, glasses, and the default body independent slot/placeholder keys so every individually visible piece composes.",
                    );
                    return;
                }
            }
        }
    }
}

fn first_invisible_cosmetic_sample(
    asset: &Arc<SkeletonAsset>,
    layers: &[spinal::SkinId; 3],
) -> Option<(String, Vec<String>)> {
    let setup_missing = invisible_cosmetic_layers(asset, layers, None);
    if !setup_missing.is_empty() {
        return Some(("setup pose".to_owned(), setup_missing));
    }
    for &name in REQUIRED_ANIMATIONS {
        let Some(id) = asset.animation_id(name) else {
            continue;
        };
        let duration = asset
            .animation(id)
            .expect("profile animation ID belongs to the asset")
            .duration();
        for (label, position) in [
            ("start", Duration::ZERO),
            (
                "midpoint",
                Duration::from_secs_f64(duration.as_secs_f64() / 2.0),
            ),
            ("endpoint", duration),
        ] {
            let missing = invisible_cosmetic_layers(asset, layers, Some((id, position)));
            if !missing.is_empty() {
                return Some((format!("animation `{name}` {label}"), missing));
            }
        }
    }
    None
}

fn invisible_cosmetic_layers(
    asset: &Arc<SkeletonAsset>,
    layers: &[spinal::SkinId; 3],
    animation: Option<(AnimationId, Duration)>,
) -> Vec<String> {
    let baseline = visible_attachments_at(asset, &[], animation);
    let composed = visible_attachments_at(asset, layers, animation);
    let mut missing = BTreeSet::new();
    for attachment in baseline {
        if !composed.contains(&attachment) {
            let name = asset
                .attachment(attachment)
                .expect("a visible attachment belongs to the frame asset")
                .name();
            missing.insert(format!("default attachment `{name}`"));
        }
    }
    for &skin_id in layers {
        let skin = asset
            .skin(skin_id)
            .expect("profile skin ID belongs to the asset");
        let own = skin
            .attachments()
            .map(|attachment| attachment.id())
            .collect::<Vec<_>>();
        let individually_visible = visible_attachments_at(asset, &[skin_id], animation)
            .into_iter()
            .filter(|attachment| own.contains(attachment))
            .collect::<Vec<_>>();
        if individually_visible.is_empty() {
            missing.insert(format!("skin `{}`", skin.name()));
        }
        for attachment in individually_visible {
            if !composed.contains(&attachment) {
                let name = asset
                    .attachment(attachment)
                    .expect("a visible attachment belongs to the frame asset")
                    .name();
                missing.insert(format!("skin `{}` attachment `{name}`", skin.name()));
            }
        }
    }
    missing.into_iter().collect()
}

fn visible_attachments_at(
    asset: &Arc<SkeletonAsset>,
    layers: &[spinal::SkinId],
    animation: Option<(AnimationId, Duration)>,
) -> Vec<AttachmentId> {
    let mut skeleton = Skeleton::new(Arc::clone(asset));
    if !layers.is_empty() {
        skeleton
            .set_skin_layers(layers)
            .expect("profile skin IDs belong to the skeleton");
    }
    if let Some((animation, position)) = animation {
        skeleton
            .sample_animation(animation, position, PlaybackMode::Once)
            .expect("profile animation ID belongs to the skeleton");
    }
    let frame = skeleton.editable_pose().solve();
    frame_facts(&frame).visible_attachments
}

fn check_optional_layers(asset: &SkeletonAsset, report: &mut Report) {
    for &name in OPTIONAL_LAYERS {
        let Some(id) = asset.animation_id(name) else {
            continue;
        };
        let animation = asset
            .animation(id)
            .expect("an animation ID resolved by this asset belongs to it");
        if !animation.override_compatibility().is_supported() {
            report.warning(
                "optional-layer-not-override-compatible",
                format!("animation:{name}"),
                format!("Optional layer `{name}` uses properties Spinal cannot yet apply on an override track."),
                "Keep this layer to override-compatible properties or leave it out of the demo export.",
            );
        }
    }
}

fn check_crossfades(
    asset: &Arc<SkeletonAsset>,
    animations: &[(&'static str, AnimationId)],
    report: &mut Report,
) {
    if animations.len() != REQUIRED_ANIMATIONS.len() {
        return;
    }
    for &(from_name, from) in animations {
        for &(to_name, to) in animations {
            if from == to {
                continue;
            }
            match simulate_crossfade(asset, from, to_name, to) {
                Ok(Some(risk)) => report.warning(
                    "crossfade-excessive-rotation",
                    format!("transition:{from_name}->{to_name}"),
                    format!(
                        "Bone `{}` travels {:.1} degrees in solved world orientation during Loafstead's 150ms crossfade (source phase {:.0}%, target speed {:.1}x). This may look like a full spin.",
                        risk.bone,
                        risk.degrees,
                        risk.source_phase * 100.0,
                        risk.target_speed,
                    ),
                    "Align the two clips' boundary poses or adjust authored angle branches, then preview this transition.",
                ),
                Ok(None) => {}
                Err(error) => report.error(
                    "crossfade-simulation-failed",
                    format!("transition:{from_name}->{to_name}"),
                    error,
                    "Correct the clips so Spinal can play and crossfade them with Loafstead's runtime settings.",
                ),
            }
        }
    }
}

fn simulate_crossfade(
    asset: &Arc<SkeletonAsset>,
    from: AnimationId,
    to_name: &str,
    to: AnimationId,
) -> Result<Option<SpinRisk>, String> {
    let source_duration = asset
        .animation(from)
        .map_err(|error| error.to_string())?
        .duration();
    let speeds = if to_name == "walk" {
        WALK_SPEEDS
    } else {
        NORMAL_SPEEDS
    };
    let mut worst = None::<SpinRisk>;
    for &source_phase in SOURCE_PHASES {
        for &target_speed in speeds {
            let candidate = simulate_crossfade_case(
                asset,
                from,
                to,
                source_duration,
                source_phase,
                target_speed,
            )?;
            if worst
                .as_ref()
                .is_none_or(|current| candidate.degrees > current.degrees)
            {
                worst = Some(candidate);
            }
        }
    }
    Ok(worst.filter(|risk| risk.degrees > 180.0 + 1.0e-3))
}

fn simulate_crossfade_case(
    asset: &Arc<SkeletonAsset>,
    from: AnimationId,
    to: AnimationId,
    source_duration: Duration,
    source_phase: f64,
    target_speed: f32,
) -> Result<SpinRisk, String> {
    let mut skeleton = Skeleton::new(Arc::clone(asset));
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .play(from, PlayOptions::looping())
        .map_err(|error| format!("could not start source clip: {error}"))?;
    let warmup = Duration::from_secs_f64(source_duration.as_secs_f64() * source_phase);
    mixer.base_track_mut().seek_to(warmup);
    let frame = mixer
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .map_err(|error| format!("could not warm source clip: {error}"))?
        .solve();
    let mut previous = frame
        .bones()
        .map(|bone| world_rotation(bone.world_transform()))
        .collect::<Vec<_>>();
    drop(frame);
    let mut travelled = vec![0.0_f32; previous.len()];
    {
        let mut base = mixer.base_track_mut();
        base.set_speed(target_speed)
            .map_err(|error| format!("could not apply target speed: {error}"))?;
        base.play(
            to,
            PlayOptions::looping().with_transition(Transition::Crossfade(
                Crossfade::new(CROSSFADE).with_curve(MixCurve::SmoothStep),
            )),
        )
        .map_err(|error| format!("could not start target clip: {error}"))?;
    }
    for _step in 0..(CROSSFADE.as_millis() / CROSSFADE_STEP.as_millis()) {
        let frame = mixer
            .update(&mut skeleton, CROSSFADE_STEP, &mut ())
            .map_err(|error| format!("could not advance transition: {error}"))?
            .solve();
        for ((previous, travelled), current) in previous.iter_mut().zip(&mut travelled).zip(
            frame
                .bones()
                .map(|bone| world_rotation(bone.world_transform())),
        ) {
            *travelled += shortest_delta(*previous, current).abs();
            *previous = current;
        }
    }
    let Some((ordinal, radians)) = travelled
        .iter()
        .copied()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(&right.1))
    else {
        return Ok(SpinRisk {
            bone: "<none>".to_owned(),
            degrees: 0.0,
            source_phase,
            target_speed,
        });
    };
    let bone = asset
        .bones()
        .nth(ordinal)
        .map(|bone| bone.name().to_owned())
        .unwrap_or_else(|| format!("#{ordinal}"));
    Ok(SpinRisk {
        bone,
        degrees: radians.to_degrees(),
        source_phase,
        target_speed,
    })
}

fn world_rotation(transform: spinal::WorldTransform) -> f32 {
    let axis = transform.x_axis();
    axis.y.atan2(axis.x)
}

#[derive(Debug)]
struct SpinRisk {
    bone: String,
    degrees: f32,
    source_phase: f64,
    target_speed: f32,
}

fn frame_facts(frame: &spinal::SolvedFrame<'_>) -> FrameFacts {
    let asset = frame.asset();
    let mut geometry_problems = Vec::new();
    let bones = frame
        .bones()
        .enumerate()
        .map(|(ordinal, bone)| {
            let world = bone.world_transform();
            if !world.translation().is_finite()
                || !world.x_axis().is_finite()
                || !world.y_axis().is_finite()
            {
                geometry_problems.push(format!(
                    "bone `{}` has a non-finite solved transform",
                    asset
                        .bones()
                        .nth(ordinal)
                        .map(|bone| bone.name())
                        .unwrap_or("<unknown>")
                ));
            }
            let local = bone.local_transform();
            BoneSnapshot {
                name: asset
                    .bones()
                    .nth(ordinal)
                    .map(|bone| bone.name().to_owned())
                    .unwrap_or_else(|| format!("#{ordinal}")),
                translation: local.translation(),
                rotation: local.rotation().as_radians(),
                scale: local.scale(),
            }
        })
        .collect::<Vec<_>>();

    let mut draw_count = 0;
    let mut visible_attachments = Vec::new();
    for draw in frame.draw_items() {
        match draw {
            DrawItemRef::Region(region) => {
                let alpha = region.color().alpha();
                if !alpha.is_finite() {
                    geometry_problems.push("a rigid region has non-finite alpha".to_owned());
                    continue;
                }
                if alpha <= 0.0 {
                    continue;
                }
                let positions = region.positions();
                let finite = positions.iter().all(|position| position.is_finite());
                let positive_area = finite && quad_has_positive_area(positions);
                let valid_uvs = region
                    .uvs()
                    .is_some_and(|uvs| uvs.iter().all(|uv| uv.is_finite()));
                if !finite {
                    geometry_problems.push("a rigid region has non-finite positions".to_owned());
                }
                if finite && !positive_area {
                    geometry_problems.push("a rigid region has zero rendered area".to_owned());
                }
                if !valid_uvs {
                    geometry_problems
                        .push("a rigid region has unavailable or non-finite UVs".to_owned());
                }
                if finite && positive_area && valid_uvs {
                    draw_count += 1;
                    visible_attachments.push(region.attachment());
                }
            }
            DrawItemRef::Mesh(mesh) => {
                let alpha = mesh.color().alpha();
                if !alpha.is_finite() {
                    geometry_problems.push("a mesh has non-finite alpha".to_owned());
                    continue;
                }
                if alpha <= 0.0 {
                    continue;
                }
                let positions = mesh.positions();
                let finite = positions.iter().all(|position| position.is_finite());
                let valid_source_uvs = positions.len() == mesh.source_uvs().len()
                    && mesh.source_uvs().iter().all(|uv| uv.is_finite());
                let valid_indices = mesh
                    .triangles()
                    .iter()
                    .all(|index| (*index as usize) < positions.len());
                let positive_area = finite
                    && valid_indices
                    && mesh_has_positive_triangle(positions, mesh.triangles());
                let valid_page_uvs = mesh
                    .uvs()
                    .is_some_and(|uvs| uvs.into_iter().all(|uv| uv.is_finite()));
                if !finite {
                    geometry_problems.push("a mesh has non-finite solved positions".to_owned());
                }
                if !valid_source_uvs {
                    geometry_problems.push("a mesh has invalid source UVs".to_owned());
                }
                if !valid_indices {
                    geometry_problems.push("a mesh triangle index is out of range".to_owned());
                }
                if finite && valid_indices && !positive_area {
                    geometry_problems.push("a mesh has no positive-area triangle".to_owned());
                }
                if !valid_page_uvs {
                    geometry_problems
                        .push("a mesh has unavailable or non-finite page UVs".to_owned());
                }
                if finite && valid_source_uvs && valid_indices && positive_area && valid_page_uvs {
                    draw_count += 1;
                    visible_attachments.push(mesh.attachment());
                }
            }
            _future => geometry_problems.push("an unknown draw-item kind was produced".to_owned()),
        }
    }

    let mut ik_warnings = Vec::new();
    for (id, status) in frame.ik_statuses() {
        let name = asset
            .ik_constraint(id)
            .map(|constraint| constraint.name())
            .unwrap_or("<unknown>");
        if status.issue().is_some() || status.preserved_underdetermined() {
            ik_warnings.push(format!(
                "IK constraint `{name}` is singular or underdetermined"
            ));
        }
        if status.target_reach() == Some(IkTargetReach::BeyondReach) {
            ik_warnings.push(format!(
                "IK constraint `{name}` has a target beyond chain reach"
            ));
        }
    }
    let mut transform_problems = Vec::new();
    for (id, status) in frame.transform_statuses() {
        if status.is_degraded() {
            let name = asset
                .transform_constraint(id)
                .map(|constraint| constraint.name())
                .unwrap_or("<unknown>");
            transform_problems.push(format!(
                "transform constraint `{name}` is singular or underdetermined"
            ));
        }
    }
    FrameFacts {
        bones,
        draw_count,
        visible_attachments,
        geometry_problems,
        ik_warnings,
        transform_problems,
    }
}

fn inventory(asset: &SkeletonAsset) -> Inventory {
    let mut regions = 0;
    let mut weighted_meshes = 0;
    let mut unweighted_meshes = 0;
    let mut linked_meshes = 0;
    let mut mesh_vertices = 0;
    let mut mesh_influences = 0;
    for attachment in asset.attachments() {
        match attachment.kind() {
            AttachmentKind::Region => regions += 1,
            AttachmentKind::Mesh => {
                let mesh = attachment
                    .as_mesh()
                    .expect("mesh attachment kind has a typed mesh view");
                if mesh.is_weighted() {
                    weighted_meshes += 1;
                } else {
                    unweighted_meshes += 1;
                }
                if mesh.source_mesh().is_some() {
                    linked_meshes += 1;
                }
                mesh_vertices += mesh.vertex_count();
                mesh_influences += mesh
                    .vertices()
                    .map(|vertex| vertex.influences().len())
                    .sum::<usize>();
            }
            _other => {}
        }
    }
    Inventory {
        bones: asset.bones().len(),
        slots: asset.slots().len(),
        skins: asset.skins().len(),
        attachments: asset.attachments().len(),
        regions,
        weighted_meshes,
        unweighted_meshes,
        linked_meshes,
        mesh_vertices,
        mesh_influences,
        ik_constraints: asset.ik_constraints().len(),
        transform_constraints: asset.transform_constraints().len(),
        atlas_pages: asset.atlas_pages().len(),
        atlas_regions: asset.atlas_regions().len(),
        animations: asset
            .animations()
            .map(|animation| AnimationInventory {
                name: animation.name().to_owned(),
                duration_seconds: animation.duration().as_secs_f64(),
            })
            .collect(),
    }
}

#[derive(Debug)]
struct FrameFacts {
    bones: Vec<BoneSnapshot>,
    draw_count: usize,
    visible_attachments: Vec<AttachmentId>,
    geometry_problems: Vec<String>,
    ik_warnings: Vec<String>,
    transform_problems: Vec<String>,
}

fn quad_has_positive_area(positions: [Vec2; 4]) -> bool {
    triangle_area_twice(positions[0], positions[1], positions[2]) > 1.0e-6
        || triangle_area_twice(positions[0], positions[2], positions[3]) > 1.0e-6
}

fn mesh_has_positive_triangle(positions: &[Vec2], triangles: &[u32]) -> bool {
    triangles.chunks_exact(3).any(|triangle| {
        triangle_area_twice(
            positions[triangle[0] as usize],
            positions[triangle[1] as usize],
            positions[triangle[2] as usize],
        ) > 1.0e-6
    })
}

fn triangle_area_twice(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    cross.abs()
}

#[derive(Clone, Debug)]
struct BoneSnapshot {
    name: String,
    translation: Vec2,
    rotation: f32,
    scale: Vec2,
}

fn transform_discontinuous(start: &BoneSnapshot, end: &BoneSnapshot) -> bool {
    start.translation.distance(end.translation) > 0.5
        || shortest_delta(start.rotation, end.rotation).abs() > 1.0_f32.to_radians()
        || (start.scale - end.scale).abs().max_element() > 0.01
}

fn scale_sign_changed(left: Vec2, right: Vec2) -> bool {
    sign(left.x) != sign(right.x) || sign(left.y) != sign(right.y)
}

fn sign(value: f32) -> i8 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

fn shortest_delta(from: f32, to: f32) -> f32 {
    (to - from + PI).rem_euclid(TAU) - PI
}

fn concise_names(names: &[String]) -> String {
    const LIMIT: usize = 6;
    let shown = names
        .iter()
        .take(LIMIT)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > LIMIT {
        format!("{shown}, and {} more", names.len() - LIMIT)
    } else {
        shown
    }
}

fn diagnostic_scope(asset: &SkeletonAsset, scope: DiagnosticScope) -> String {
    match scope {
        DiagnosticScope::Asset => "asset".to_owned(),
        DiagnosticScope::Bone(id) => named_scope("bone", asset.bone(id).map(|item| item.name())),
        DiagnosticScope::Slot(id) => named_scope("slot", asset.slot(id).map(|item| item.name())),
        DiagnosticScope::Skin(id) => named_scope("skin", asset.skin(id).map(|item| item.name())),
        DiagnosticScope::Animation(id) => {
            named_scope("animation", asset.animation(id).map(|item| item.name()))
        }
        DiagnosticScope::Event(id) => {
            named_scope("event", asset.event_definition(id).map(|item| item.name()))
        }
        DiagnosticScope::Attachment(id) => {
            named_scope("attachment", asset.attachment(id).map(|item| item.name()))
        }
        DiagnosticScope::IkConstraint(id) => named_scope(
            "ik-constraint",
            asset.ik_constraint(id).map(|item| item.name()),
        ),
        DiagnosticScope::Constraint(id) => {
            named_scope("constraint", asset.constraint(id).map(|item| item.name()))
        }
        DiagnosticScope::AtlasPage(id) => {
            named_scope("atlas-page", asset.atlas_page(id).map(|item| item.name()))
        }
        DiagnosticScope::AtlasRegion(id) => named_scope(
            "atlas-region",
            asset.atlas_region(id).map(|item| item.name()),
        ),
        _future => "future".to_owned(),
    }
}

fn named_scope(kind: &str, name: Result<&str, spinal::IdError>) -> String {
    format!("{kind}:{}", name.unwrap_or("<invalid-id>"))
}

fn diagnostic_code_name(code: DiagnosticCode) -> &'static str {
    match code {
        DiagnosticCode::UnsupportedAttachmentType => "unsupported-attachment-type",
        DiagnosticCode::UnsupportedConstraintType => "unsupported-constraint-type",
        DiagnosticCode::UnsupportedConstraintOption => "unsupported-constraint-option",
        DiagnosticCode::UnsupportedBoneTransformMode => "unsupported-bone-transform-mode",
        DiagnosticCode::UnsupportedTimelineType => "unsupported-timeline-type",
        DiagnosticCode::UnsupportedBlendMode => "unsupported-blend-mode",
        DiagnosticCode::UnsupportedTwoColourTint => "unsupported-two-colour-tint",
        DiagnosticCode::IgnoredSkinBones => "ignored-skin-bones",
        DiagnosticCode::IgnoredSkinConstraints => "ignored-skin-constraints",
        DiagnosticCode::UnknownField => "unknown-field",
        DiagnosticCode::UntestedPatchVersion => "untested-patch-version",
        DiagnosticCode::AlphaEncodingMismatch => "alpha-encoding-mismatch",
        DiagnosticCode::UnsupportedAtlasSetting => "unsupported-atlas-setting",
        DiagnosticCode::UnsupportedAtlasRotation => "unsupported-atlas-rotation",
        DiagnosticCode::DiagnosticsTruncated => "diagnostics-truncated",
        _future => "future",
    }
}

fn load_error_name(kind: LoadErrorKind) -> &'static str {
    match kind {
        LoadErrorKind::InvalidUtf8 => "invalid-utf8",
        LoadErrorKind::Syntax => "syntax",
        LoadErrorKind::SchemaViolation => "schema-violation",
        LoadErrorKind::InvalidVersion => "invalid-version",
        LoadErrorKind::UnsupportedVersion => "unsupported-version",
        LoadErrorKind::NonFiniteNumber => "non-finite-number",
        LoadErrorKind::DuplicateField => "duplicate-field",
        LoadErrorKind::DuplicateName => "duplicate-name",
        LoadErrorKind::InvalidOrder => "invalid-order",
        LoadErrorKind::InvalidTopology => "invalid-topology",
        LoadErrorKind::UnresolvedReference => "unresolved-reference",
        LoadErrorKind::MissingAtlasRegion => "missing-atlas-region",
        LoadErrorKind::AmbiguousAtlasRegion => "ambiguous-atlas-region",
        LoadErrorKind::UnsupportedData => "unsupported-data",
        LoadErrorKind::CapacityExceeded => "capacity-exceeded",
        _future => "future",
    }
}

fn load_error_scope(error: &spinal::LoadError) -> String {
    let document = match error.location().document() {
        LoadDocument::SkeletonJson => "skeleton-json",
        LoadDocument::Atlas => "atlas",
        _future => "future",
    };
    match error.path() {
        Some(path) => format!("{document}:{path}"),
        None => document.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_sweep_detects_parent_child_world_spin_only_at_fast_walk_speed() {
        let asset = transition_asset(
            r#"[
                {"value":0},
                {"time":1,"value":0}
            ]"#,
            r#"[
                {"value":0},
                {"time":1,"value":240}
            ]"#,
        );
        let idle = asset.animation_id("idle").expect("idle animation");
        let walk = asset.animation_id("walk").expect("walk animation");

        let risk = simulate_crossfade(&asset, idle, "walk", walk)
            .expect("transition simulation")
            .expect("world-space spin risk");

        assert_eq!(risk.target_speed, 3.0);
        assert_eq!(risk.bone, "child");
        assert!(risk.degrees > 180.0, "{risk:?}");
    }

    #[test]
    fn transition_sweep_checks_source_phases_beyond_the_midpoint() {
        let asset = transition_asset(
            r#"[
                {"value":0},
                {"time":0.25,"value":120},
                {"time":0.5,"value":0},
                {"time":1,"value":0}
            ]"#,
            r#"[
                {"value":0},
                {"time":1,"value":0}
            ]"#,
        );
        let idle = asset.animation_id("idle").expect("idle animation");
        let walk = asset.animation_id("walk").expect("walk animation");

        let risk = simulate_crossfade(&asset, idle, "idle", walk)
            .expect("transition simulation")
            .expect("phase-specific world-space spin risk");

        assert_eq!(risk.source_phase, 0.25);
        assert_eq!(risk.bone, "child");
    }

    #[test]
    fn collinear_triangle_has_no_rendered_area() {
        assert_eq!(
            triangle_area_twice(Vec2::ZERO, Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0)),
            0.0
        );
    }

    fn transition_asset(source_frames: &str, target_frames: &str) -> Arc<SkeletonAsset> {
        let json = format!(
            r#"{{
              "skeleton":{{"spine":"4.3.23"}},
              "bones":[
                {{"name":"parent"}},
                {{"name":"child","parent":"parent"}}
              ],
              "animations":{{
                "idle":{{"bones":{{
                  "parent":{{"rotate":{source_frames}}},
                  "child":{{"rotate":{source_frames}}}
                }}}},
                "walk":{{"bones":{{
                  "parent":{{"rotate":{target_frames}}},
                  "child":{{"rotate":{target_frames}}}
                }}}}
              }}
            }}"#
        );
        spinal::load_json(json.as_bytes(), b"page.png\n")
            .expect("transition fixture loads")
            .into_asset()
    }
}

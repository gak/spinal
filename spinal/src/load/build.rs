use std::collections::{HashMap, HashSet};

use glam::Vec2;

use crate::{
    Angle, BendDirection, BoneTransform, DiagnosticCode, Mix, PixelSize, Rgba8, Shear,
    SlotBlendMode, TARGET_SPINE_MAJOR, TARGET_SPINE_MINOR, TARGET_SPINE_VERSION, TransformMix,
    animation::{EventDefinitionData, EventPayload},
    asset::{
        AssetData, AtlasExtension as AssetAtlasExtension, AtlasPageData, AtlasRegionData,
        AttachmentData, AttachmentDataKind, BoneData, ConstraintData, IkConstraintData,
        RegionAttachmentData, SkinData, SlotData, TransformConstraintData,
        TransformConstraintPoseData,
    },
    atlas::{AtlasIssueKind, AtlasIssueTarget, ParsedAtlas, ParsedAtlasPage, ParsedAtlasRegion},
    id::AssetKey,
    json::{JsonMember, JsonValue},
    mesh::{MeshAttachmentData, MeshGeometryData},
};

use super::{
    LoadDocument, LoadError, LoadErrorKind, PendingDiagnostic, PendingDiagnostics, PendingScope,
    SourceLocation,
    animation::{AnimationLinks, parse_animations},
    mesh::{
        PendingLinkedMesh, parse_mesh_geometry, resolve_attachment_atlas_region,
        resolve_linked_meshes,
    },
    schema::{
        array, bool_or, colour_or, error, f32_or, finite_f32, i32_value, index_pointer, member,
        nonempty_string, object, optional_nonempty_string, optional_string, pointer,
        required_member, schema_error, string, u32_or, u32_value, unique_members,
    },
};

pub(crate) fn build_asset(
    root: &JsonValue,
    atlas: ParsedAtlas,
) -> Result<(AssetKey, AssetData), LoadError> {
    let root = object(root, "")?;
    unique_members(root, "")?;
    let mut pending = PendingDiagnostics::new();

    let spine_version = parse_version(root, &mut pending)?;
    diagnose_unknown_root_fields(root, &mut pending);
    let (atlas_pages, atlas_regions, atlas_by_name) = convert_atlas(atlas, &mut pending)?;
    let (bones, bone_by_name) = parse_bones(root, &mut pending)?;
    let (slots, slot_by_name) = parse_slots(root, &bone_by_name, &mut pending)?;
    let (skins, attachments, mesh_geometries, _skin_by_name) = parse_skins(
        root,
        &slot_by_name,
        bones.len(),
        &atlas_by_name,
        &atlas_regions,
        &mut pending,
    )?;
    let (_attachments_by_skin_slot, attachment_names_by_slot) = index_attachments(&attachments)?;
    validate_setup_attachments(&slots, &attachment_names_by_slot)?;
    let (constraints, ik_constraints, ik_by_name, transform_constraints, transform_by_name) =
        parse_constraints(root, &bones, &bone_by_name, &mut pending)?;
    let (events, event_by_name) = parse_events(root, &mut pending)?;
    let animations = parse_animations(
        member(root, "animations", "")?,
        AnimationLinks {
            bones: &bone_by_name,
            slots: &slot_by_name,
            ik_constraints: &ik_by_name,
            ik_constraint_data: &ik_constraints,
            transform_constraints: &transform_by_name,
            transform_constraint_data: &transform_constraints,
            events: &event_by_name,
            event_definitions: &events,
            attachment_names: &attachment_names_by_slot,
        },
        &mut pending,
    )?;

    ensure_capacity(bones.len(), "/bones")?;
    ensure_capacity(slots.len(), "/slots")?;
    ensure_capacity(skins.len(), "/skins")?;
    ensure_capacity(attachments.len(), "/skins")?;
    ensure_capacity(mesh_geometries.len(), "/skins")?;
    ensure_capacity(animations.len(), "/animations")?;
    ensure_capacity(ik_constraints.len(), "/constraints")?;
    ensure_capacity(transform_constraints.len(), "/constraints")?;
    ensure_capacity(constraints.len(), "/constraints")?;
    ensure_capacity(atlas_pages.len(), "/pages")?;
    ensure_capacity(atlas_regions.len(), "/regions")?;
    ensure_capacity(events.len(), "/events")?;

    let key = AssetKey::try_fresh().ok_or_else(|| {
        LoadError::new(
            LoadErrorKind::CapacityExceeded,
            "process-local asset identity space is exhausted",
            SourceLocation::for_document(LoadDocument::SkeletonJson),
        )
    })?;
    let diagnostics = pending
        .into_iter()
        .map(|diagnostic| diagnostic.materialize(key))
        .collect();

    Ok((
        key,
        AssetData {
            spine_version,
            bones,
            slots,
            skins,
            attachments,
            mesh_geometries,
            animations,
            ik_constraints,
            transform_constraints,
            constraints,
            atlas_pages,
            atlas_regions,
            events,
            diagnostics,
        },
    ))
}

fn diagnose_unknown_root_fields(root: &[JsonMember], pending: &mut PendingDiagnostics) {
    for field in root {
        if matches!(
            field.name(),
            "skeleton"
                | "bones"
                | "slots"
                | "skins"
                | "constraints"
                | "ik"
                | "transform"
                | "path"
                | "physics"
                | "events"
                | "animations"
        ) {
            continue;
        }
        pending.push(PendingDiagnostic::degraded(
            DiagnosticCode::UnknownField,
            PendingScope::Asset,
            format!(
                "unknown top-level skeleton section {:?} was retained only as unsupported data",
                field.name()
            ),
        ));
    }
}

fn parse_version(
    root: &[JsonMember],
    pending: &mut PendingDiagnostics,
) -> Result<Box<str>, LoadError> {
    let metadata_value = required_member(root, "skeleton", "")?;
    let metadata = object(metadata_value, "/skeleton")?;
    unique_members(metadata, "/skeleton")?;
    for field in metadata {
        if !matches!(
            field.name(),
            "hash"
                | "spine"
                | "version"
                | "x"
                | "y"
                | "width"
                | "height"
                | "fps"
                | "images"
                | "audio"
        ) {
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::UnknownField,
                PendingScope::Asset,
                format!(
                    "skeleton metadata contains unknown field {:?}",
                    field.name()
                ),
            ));
        }
    }
    let spine = member(metadata, "spine", "/skeleton")?;
    let version_alias = member(metadata, "version", "/skeleton")?;
    let version_value = match (spine, version_alias) {
        (Some(_), Some(_)) => {
            return Err(schema_error(
                "/skeleton",
                "version is specified by both \"spine\" and \"version\"",
            ));
        }
        (Some(value), None) => value,
        (None, Some(value)) => value,
        (None, None) => {
            return Err(error(
                LoadErrorKind::InvalidVersion,
                "/skeleton/spine",
                "skeleton export version is missing",
            ));
        }
    };
    let version_path = if spine.is_some() {
        "/skeleton/spine"
    } else {
        "/skeleton/version"
    };
    let version = string(version_value, version_path)?;

    if version.contains('-') {
        return Err(error(
            LoadErrorKind::UnsupportedVersion,
            version_path,
            format!("prerelease Spine export {version:?} is not supported"),
        ));
    }
    let components = version.split('.').collect::<Vec<_>>();
    if components.len() != 3
        || components.iter().any(|component| {
            component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(error(
            LoadErrorKind::InvalidVersion,
            version_path,
            format!("Spine version {version:?} is not a major.minor.patch version"),
        ));
    }
    let major = components[0].parse::<u16>().map_err(|_error| {
        error(
            LoadErrorKind::InvalidVersion,
            version_path,
            "Spine major version is outside the supported numeric range",
        )
    })?;
    let minor = components[1].parse::<u16>().map_err(|_error| {
        error(
            LoadErrorKind::InvalidVersion,
            version_path,
            "Spine minor version is outside the supported numeric range",
        )
    })?;
    let _patch = components[2].parse::<u32>().map_err(|_error| {
        error(
            LoadErrorKind::InvalidVersion,
            version_path,
            "Spine patch version is outside the supported numeric range",
        )
    })?;

    if major != TARGET_SPINE_MAJOR || minor != TARGET_SPINE_MINOR {
        return Err(error(
            LoadErrorKind::UnsupportedVersion,
            version_path,
            format!(
                "Spine {version} is incompatible with the {}.{} target",
                TARGET_SPINE_MAJOR, TARGET_SPINE_MINOR
            ),
        ));
    }
    if version != TARGET_SPINE_VERSION {
        pending.push(PendingDiagnostic::warning(
            DiagnosticCode::UntestedPatchVersion,
            PendingScope::Asset,
            format!(
                "Spine {version} has not passed the exact {TARGET_SPINE_VERSION} conformance suite"
            ),
        ));
    }
    Ok(version.into())
}

type AtlasLookup = HashMap<Box<str>, Vec<u32>>;
type AtlasParse = (Box<[AtlasPageData]>, Box<[AtlasRegionData]>, AtlasLookup);

fn convert_atlas(
    atlas: ParsedAtlas,
    pending: &mut PendingDiagnostics,
) -> Result<AtlasParse, LoadError> {
    ensure_capacity(atlas.pages.len(), "/pages")?;
    ensure_capacity(atlas.regions.len(), "/regions")?;

    for issue in &atlas.issues {
        let (code, scope) = match (issue.kind(), issue.target()) {
            (AtlasIssueKind::PremultipliedAlpha, AtlasIssueTarget::Page(index)) => (
                DiagnosticCode::AlphaEncodingMismatch,
                PendingScope::AtlasPage(index_u32(index, "/pages")?),
            ),
            (AtlasIssueKind::UnsupportedPageSetting, AtlasIssueTarget::Page(index)) => (
                DiagnosticCode::UnsupportedAtlasSetting,
                PendingScope::AtlasPage(index_u32(index, "/pages")?),
            ),
            (AtlasIssueKind::UnsupportedRotation, AtlasIssueTarget::Region(index)) => (
                DiagnosticCode::UnsupportedAtlasRotation,
                PendingScope::AtlasRegion(index_u32(index, "/regions")?),
            ),
            (AtlasIssueKind::PremultipliedAlpha, AtlasIssueTarget::Region(index))
            | (AtlasIssueKind::UnsupportedPageSetting, AtlasIssueTarget::Region(index))
            | (AtlasIssueKind::UnsupportedRotation, AtlasIssueTarget::Page(index)) => {
                return Err(error(
                    LoadErrorKind::SchemaViolation,
                    "/",
                    format!("atlas parser produced an invalid issue target at index {index}"),
                ));
            }
        };
        pending.push(PendingDiagnostic::degraded(code, scope, issue.message()));
    }

    let pages = atlas
        .pages
        .into_iter()
        .map(convert_atlas_page)
        .collect::<Result<Box<_>, _>>()?;
    let mut lookup = HashMap::<Box<str>, Vec<u32>>::new();
    let mut regions = Vec::with_capacity(atlas.regions.len());
    for (index, region) in atlas.regions.into_iter().enumerate() {
        let index_u32 = index_u32(index, "/regions")?;
        lookup
            .entry(region.name.clone())
            .or_default()
            .push(index_u32);
        regions.push(convert_atlas_region(region)?);
    }
    Ok((pages, regions.into_boxed_slice(), lookup))
}

fn convert_atlas_page(page: ParsedAtlasPage) -> Result<AtlasPageData, LoadError> {
    Ok(AtlasPageData {
        name: page.name,
        size: page.size,
        format: page.format,
        format_token: page.format_token,
        min_filter: page.min_filter,
        min_filter_token: page.min_filter_token,
        mag_filter: page.mag_filter,
        mag_filter_token: page.mag_filter_token,
        wrap: page.wrap,
        alpha_encoding: page.alpha_encoding,
        scale: page.scale,
        regions: index_u32(page.region_range.start, "/pages")?
            ..index_u32(page.region_range.end, "/pages")?,
        extensions: page
            .extensions
            .into_iter()
            .map(|extension| AssetAtlasExtension {
                key: extension.key,
                value: extension.value,
            })
            .collect(),
    })
}

fn convert_atlas_region(region: ParsedAtlasRegion) -> Result<AtlasRegionData, LoadError> {
    Ok(AtlasRegionData {
        name: region.name,
        page: index_u32(region.page, "/regions")?,
        index: region.index,
        bounds: region.bounds,
        trim: region.offsets,
        rotation: region.rotation,
        split: region.split,
        pad: region.pad,
        extensions: region
            .extensions
            .into_iter()
            .map(|extension| AssetAtlasExtension {
                key: extension.key,
                value: extension.value,
            })
            .collect(),
    })
}

type BoneParse = (Box<[BoneData]>, HashMap<Box<str>, u32>);

fn parse_bones(
    root: &[JsonMember],
    pending: &mut PendingDiagnostics,
) -> Result<BoneParse, LoadError> {
    let values = array(required_member(root, "bones", "")?, "/bones")?;
    if values.is_empty() {
        return Err(schema_error(
            "/bones",
            "skeleton must contain at least one bone",
        ));
    }
    ensure_capacity(values.len(), "/bones")?;

    let mut names = HashMap::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let path = index_pointer("/bones", index);
        let bone = object(value, &path)?;
        unique_members(bone, &path)?;
        let name_path = pointer(&path, "name");
        let name = nonempty_string(required_member(bone, "name", &path)?, &name_path)?;
        if names
            .insert(Box::from(name), index_u32(index, &path)?)
            .is_some()
        {
            return Err(error(
                LoadErrorKind::DuplicateName,
                &name_path,
                format!("bone name {name:?} is duplicated"),
            ));
        }
    }

    let mut bones = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let path = index_pointer("/bones", index);
        let bone = object(value, &path)?;
        let name = nonempty_string(
            required_member(bone, "name", &path)?,
            &pointer(&path, "name"),
        )?;
        let parent_name = optional_string(bone, "parent", &path)?;
        let parent = match parent_name {
            None if index == 0 => None,
            None => {
                return Err(error(
                    LoadErrorKind::InvalidTopology,
                    &pointer(&path, "parent"),
                    "only the first bone may omit its parent",
                ));
            }
            Some(parent_name) => {
                let parent_index = names.get(parent_name).copied().ok_or_else(|| {
                    error(
                        LoadErrorKind::UnresolvedReference,
                        &pointer(&path, "parent"),
                        format!("bone parent {parent_name:?} does not exist"),
                    )
                })?;
                if parent_index >= index_u32(index, &path)? {
                    return Err(error(
                        LoadErrorKind::InvalidTopology,
                        &pointer(&path, "parent"),
                        "a bone parent must precede its child",
                    ));
                }
                Some(parent_index)
            }
        };

        let translation = Vec2::new(
            f32_or(bone, "x", &path, 0.0)?,
            f32_or(bone, "y", &path, 0.0)?,
        );
        let rotation = Angle::from_degrees(f32_or(bone, "rotation", &path, 0.0)?)
            .map_err(|_error| nonfinite_transform_error(&path, "rotation"))?;
        let scale = Vec2::new(
            f32_or(bone, "scaleX", &path, 1.0)?,
            f32_or(bone, "scaleY", &path, 1.0)?,
        );
        let shear = Shear::from_degrees(
            f32_or(bone, "shearX", &path, 0.0)?,
            f32_or(bone, "shearY", &path, 0.0)?,
        )
        .map_err(|_error| nonfinite_transform_error(&path, "shear"))?;
        let setup_transform = BoneTransform::new(translation, rotation, scale, shear)
            .map_err(|_error| nonfinite_transform_error(&path, "transform"))?;

        let transform = optional_string(bone, "transform", &path)?;
        let inherit = optional_string(bone, "inherit", &path)?;
        if transform.is_some() && inherit.is_some() {
            return Err(schema_error(
                &path,
                "bone inheritance is specified by both \"transform\" and \"inherit\"",
            ));
        }
        let inheritance = transform.or(inherit).unwrap_or("normal");
        let bone_index = index_u32(index, &path)?;
        if inheritance != "normal" {
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::UnsupportedBoneTransformMode,
                PendingScope::Bone(bone_index),
                format!("bone {name:?} uses unsupported inheritance mode {inheritance:?}"),
            ));
        }
        if bool_or(bone, "skin", &path, false)? {
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::IgnoredSkinBones,
                PendingScope::Bone(bone_index),
                format!("bone {name:?} is active only for a skin"),
            ));
        }
        if let Some(colour) = member(bone, "color", &path)? {
            let _colour = crate::load::schema::colour(colour, &pointer(&path, "color"))?;
        }
        diagnose_unknown_record_fields(
            bone,
            &[
                "name",
                "parent",
                "length",
                "transform",
                "inherit",
                "skin",
                "x",
                "y",
                "rotation",
                "scaleX",
                "scaleY",
                "shearX",
                "shearY",
                "color",
                "icon",
            ],
            PendingScope::Bone(bone_index),
            "bone",
            name,
            pending,
        );

        bones.push(BoneData {
            name: name.into(),
            parent,
            length: f32_or(bone, "length", &path, 0.0)?,
            setup_transform,
        });
    }
    Ok((bones.into_boxed_slice(), names))
}

fn nonfinite_transform_error(path: &str, component: &str) -> LoadError {
    error(
        LoadErrorKind::NonFiniteNumber,
        path,
        format!("bone {component} must be finite"),
    )
}

type SlotParse = (Box<[SlotData]>, HashMap<Box<str>, u32>);

fn parse_slots(
    root: &[JsonMember],
    bones: &HashMap<Box<str>, u32>,
    pending: &mut PendingDiagnostics,
) -> Result<SlotParse, LoadError> {
    let Some(value) = member(root, "slots", "")? else {
        return Ok((Box::default(), HashMap::new()));
    };
    let values = array(value, "/slots")?;
    ensure_capacity(values.len(), "/slots")?;
    let mut slots = Vec::with_capacity(values.len());
    let mut names = HashMap::with_capacity(values.len());

    for (index, value) in values.iter().enumerate() {
        let path = index_pointer("/slots", index);
        let slot = object(value, &path)?;
        unique_members(slot, &path)?;
        let name = nonempty_string(
            required_member(slot, "name", &path)?,
            &pointer(&path, "name"),
        )?;
        if names
            .insert(Box::from(name), index_u32(index, &path)?)
            .is_some()
        {
            return Err(error(
                LoadErrorKind::DuplicateName,
                &pointer(&path, "name"),
                format!("slot name {name:?} is duplicated"),
            ));
        }
        let bone_name = nonempty_string(
            required_member(slot, "bone", &path)?,
            &pointer(&path, "bone"),
        )?;
        let bone = bones.get(bone_name).copied().ok_or_else(|| {
            error(
                LoadErrorKind::UnresolvedReference,
                &pointer(&path, "bone"),
                format!("slot bone {bone_name:?} does not exist"),
            )
        })?;
        let blend_token = optional_string(slot, "blend", &path)?.unwrap_or("normal");
        let blend_mode = match blend_token {
            "normal" => SlotBlendMode::Normal,
            "additive" => SlotBlendMode::Additive,
            "multiply" => SlotBlendMode::Multiply,
            "screen" => SlotBlendMode::Screen,
            _ => SlotBlendMode::Unknown,
        };
        let slot_index = index_u32(index, &path)?;
        if blend_mode != SlotBlendMode::Normal {
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::UnsupportedBlendMode,
                PendingScope::Slot(slot_index),
                format!("slot {name:?} uses unsupported blend mode {blend_token:?}"),
            ));
        }
        if let Some(dark) = member(slot, "dark", &path)? {
            let _dark = crate::load::schema::colour(dark, &pointer(&path, "dark"))?;
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::UnsupportedTwoColourTint,
                PendingScope::Slot(slot_index),
                format!("slot {name:?} uses unsupported two-colour tint"),
            ));
        }
        diagnose_unknown_record_fields(
            slot,
            &["name", "bone", "attachment", "color", "dark", "blend"],
            PendingScope::Slot(slot_index),
            "slot",
            name,
            pending,
        );

        slots.push(SlotData {
            name: name.into(),
            bone,
            setup_attachment_name: optional_string(slot, "attachment", &path)?.map(Box::from),
            colour: colour_or(slot, "color", &path, Rgba8::WHITE)?,
            blend_mode,
            blend_token: blend_token.into(),
        });
    }
    Ok((slots.into_boxed_slice(), names))
}

type SkinParse = (
    Box<[SkinData]>,
    Box<[AttachmentData]>,
    Box<[MeshGeometryData]>,
    HashMap<Box<str>, u32>,
);

fn parse_skins(
    root: &[JsonMember],
    slots: &HashMap<Box<str>, u32>,
    bone_count: usize,
    atlas: &AtlasLookup,
    atlas_regions: &[AtlasRegionData],
    pending: &mut PendingDiagnostics,
) -> Result<SkinParse, LoadError> {
    let Some(value) = member(root, "skins", "")? else {
        return Ok((
            Box::default(),
            Box::default(),
            Box::default(),
            HashMap::new(),
        ));
    };
    let values = array(value, "/skins")?;
    ensure_capacity(values.len(), "/skins")?;
    let mut skins = Vec::with_capacity(values.len());
    let mut attachments = Vec::new();
    let mut mesh_geometries = Vec::new();
    let mut linked_meshes = Vec::new();
    let mut names = HashMap::with_capacity(values.len());

    for (skin_index, value) in values.iter().enumerate() {
        let path = index_pointer("/skins", skin_index);
        let skin = object(value, &path)?;
        unique_members(skin, &path)?;
        let name = nonempty_string(
            required_member(skin, "name", &path)?,
            &pointer(&path, "name"),
        )?;
        let skin_index_u32 = index_u32(skin_index, &path)?;
        if names.insert(Box::from(name), skin_index_u32).is_some() {
            return Err(error(
                LoadErrorKind::DuplicateName,
                &pointer(&path, "name"),
                format!("skin name {name:?} is duplicated"),
            ));
        }
        diagnose_skin_membership(skin, name, skin_index_u32, &path, pending)?;
        diagnose_unknown_record_fields(
            skin,
            &[
                "name",
                "attachments",
                "bones",
                "ik",
                "transform",
                "path",
                "physics",
                "constraints",
            ],
            PendingScope::Skin(skin_index_u32),
            "skin",
            name,
            pending,
        );
        let start = index_u32(attachments.len(), &path)?;

        if let Some(value) = member(skin, "attachments", &path)? {
            let attachments_object = object(value, &pointer(&path, "attachments"))?;
            unique_members(attachments_object, &pointer(&path, "attachments"))?;
            for slot_member in attachments_object {
                let slot_path = pointer(&pointer(&path, "attachments"), slot_member.name());
                let slot = slots.get(slot_member.name()).copied().ok_or_else(|| {
                    error(
                        LoadErrorKind::UnresolvedReference,
                        &slot_path,
                        format!("skin slot {:?} does not exist", slot_member.name()),
                    )
                })?;
                let attachment_object = object(slot_member.value(), &slot_path)?;
                unique_members(attachment_object, &slot_path)?;
                for attachment_member in attachment_object {
                    let attachment_path = pointer(&slot_path, attachment_member.name());
                    if attachment_member.name().is_empty() {
                        return Err(schema_error(
                            &attachment_path,
                            "attachment placeholder name must not be empty",
                        ));
                    }
                    let attachment_index = index_u32(attachments.len(), &attachment_path)?;
                    let data = parse_attachment(
                        attachment_member.name(),
                        attachment_member.value(),
                        &attachment_path,
                        skin_index_u32,
                        slot,
                        bone_count,
                        atlas,
                        atlas_regions,
                        attachment_index,
                        &mut mesh_geometries,
                        &mut linked_meshes,
                        pending,
                    )?;
                    attachments.push(data);
                }
            }
        }
        let end = index_u32(attachments.len(), &path)?;
        skins.push(SkinData {
            name: name.into(),
            attachments: start..end,
        });
    }
    resolve_linked_meshes(&mut attachments, &names, &linked_meshes)?;
    Ok((
        skins.into_boxed_slice(),
        attachments.into_boxed_slice(),
        mesh_geometries.into_boxed_slice(),
        names,
    ))
}

fn diagnose_skin_membership(
    skin: &[JsonMember],
    name: &str,
    skin_index: u32,
    path: &str,
    pending: &mut PendingDiagnostics,
) -> Result<(), LoadError> {
    if member_is_nonempty(skin, "bones", path)? {
        pending.push(PendingDiagnostic::degraded(
            DiagnosticCode::IgnoredSkinBones,
            PendingScope::Skin(skin_index),
            format!("skin {name:?} contains unsupported skin-specific bones"),
        ));
    }
    for field in ["ik", "transform", "path", "physics", "constraints"] {
        if member_is_nonempty(skin, field, path)? {
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::IgnoredSkinConstraints,
                PendingScope::Skin(skin_index),
                format!("skin {name:?} contains unsupported {field} membership"),
            ));
        }
    }
    Ok(())
}

fn member_is_nonempty(object: &[JsonMember], field: &str, path: &str) -> Result<bool, LoadError> {
    let Some(value) = member(object, field, path)? else {
        return Ok(false);
    };
    match value {
        JsonValue::Array(values) => Ok(!values.is_empty()),
        JsonValue::Object(values) => Ok(!values.is_empty()),
        JsonValue::Null => Ok(false),
        _ => Err(schema_error(
            &pointer(path, field),
            format!("{field} membership must be an array or object"),
        )),
    }
}

fn diagnose_unknown_record_fields(
    object: &[JsonMember],
    known: &[&str],
    scope: PendingScope,
    record_type: &str,
    record_name: &str,
    pending: &mut PendingDiagnostics,
) {
    for member in object {
        if !known.contains(&member.name()) {
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::UnknownField,
                scope,
                format!(
                    "{record_type} {record_name:?} contains unknown field {:?}",
                    member.name()
                ),
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_attachment(
    placeholder_name: &str,
    value: &JsonValue,
    path: &str,
    skin: u32,
    slot: u32,
    bone_count: usize,
    atlas: &AtlasLookup,
    atlas_regions: &[AtlasRegionData],
    attachment_index: u32,
    mesh_geometries: &mut Vec<MeshGeometryData>,
    linked_meshes: &mut Vec<PendingLinkedMesh>,
    pending: &mut PendingDiagnostics,
) -> Result<AttachmentData, LoadError> {
    let attachment = object(value, path)?;
    unique_members(attachment, path)?;
    let source_type = optional_string(attachment, "type", path)?.unwrap_or("region");
    let explicit_name = optional_nonempty_string(attachment, "name", path)?;
    let atlas_path = optional_nonempty_string(attachment, "path", path)?.map(Box::from);
    let actual_name = explicit_name.unwrap_or(placeholder_name);
    let lookup_name = atlas_path.as_deref().unwrap_or(actual_name);
    let unknown_region_field = attachment.iter().find(|member| {
        !matches!(
            member.name(),
            "type"
                | "name"
                | "path"
                | "x"
                | "y"
                | "rotation"
                | "scaleX"
                | "scaleY"
                | "width"
                | "height"
                | "color"
                | "sequence"
        )
    });
    let unknown_mesh_field = attachment.iter().find(|member| {
        !matches!(
            member.name(),
            "type"
                | "name"
                | "path"
                | "color"
                | "uvs"
                | "triangles"
                | "vertices"
                | "hull"
                | "edges"
                | "width"
                | "height"
                | "sequence"
        )
    });
    let unknown_linked_mesh_field = attachment.iter().find(|member| {
        !matches!(
            member.name(),
            "type"
                | "name"
                | "path"
                | "color"
                | "skin"
                | "parent"
                | "deform"
                | "width"
                | "height"
                | "sequence"
        )
    });
    let sequence = member(attachment, "sequence", path)?;

    let kind = match source_type {
        "region" if sequence.is_none() && unknown_region_field.is_none() => {
            let matches = atlas.get(lookup_name).map_or(&[][..], Vec::as_slice);
            let atlas_region = match matches {
                [] => {
                    return Err(error(
                        LoadErrorKind::MissingAtlasRegion,
                        path,
                        format!("region attachment requires atlas region {lookup_name:?}"),
                    ));
                }
                [index] => *index,
                _ => {
                    return Err(error(
                        LoadErrorKind::AmbiguousAtlasRegion,
                        path,
                        format!("region attachment {lookup_name:?} matches multiple atlas regions"),
                    ));
                }
            };
            if atlas_regions.get(atlas_region as usize).is_none() {
                return Err(error(
                    LoadErrorKind::SchemaViolation,
                    path,
                    "atlas lookup produced an invalid region index",
                ));
            }
            let width = attachment_pixel_size(attachment, "width", path)?;
            let height = attachment_pixel_size(attachment, "height", path)?;
            let transform = BoneTransform::new(
                Vec2::new(
                    f32_or(attachment, "x", path, 0.0)?,
                    f32_or(attachment, "y", path, 0.0)?,
                ),
                Angle::from_degrees(f32_or(attachment, "rotation", path, 0.0)?)
                    .map_err(|_error| nonfinite_transform_error(path, "rotation"))?,
                Vec2::new(
                    f32_or(attachment, "scaleX", path, 1.0)?,
                    f32_or(attachment, "scaleY", path, 1.0)?,
                ),
                Shear::ZERO,
            )
            .map_err(|_error| nonfinite_transform_error(path, "attachment transform"))?;
            AttachmentDataKind::Region(RegionAttachmentData {
                transform,
                size: PixelSize::new(width, height),
                colour: colour_or(attachment, "color", path, Rgba8::WHITE)?,
                atlas_region,
            })
        }
        "region" => {
            let reason = if sequence.is_some() {
                "attachment sequences are outside the active profile".to_owned()
            } else {
                format!(
                    "unknown region field {:?} has no safe fallback",
                    unknown_region_field
                        .map(JsonMember::name)
                        .unwrap_or("unknown")
                )
            };
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::UnsupportedAttachmentType,
                PendingScope::Attachment(attachment_index),
                format!("region attachment {placeholder_name:?} is unsupported: {reason}"),
            ));
            AttachmentDataKind::Unsupported {
                source_type: "region".into(),
            }
        }
        "mesh" if sequence.is_none() && unknown_mesh_field.is_none() => {
            let atlas_region =
                resolve_attachment_atlas_region(atlas, atlas_regions, lookup_name, "mesh", path)?;
            let geometry = parse_mesh_geometry(attachment, path, bone_count)?;
            let geometry_index = index_u32(mesh_geometries.len(), path)?;
            mesh_geometries.push(geometry);
            AttachmentDataKind::Mesh(MeshAttachmentData {
                colour: colour_or(attachment, "color", path, Rgba8::WHITE)?,
                atlas_region,
                geometry: geometry_index,
                source_mesh: None,
                inherits_deform: false,
            })
        }
        "linkedmesh" if sequence.is_none() && unknown_linked_mesh_field.is_none() => {
            let atlas_region = resolve_attachment_atlas_region(
                atlas,
                atlas_regions,
                lookup_name,
                "linked mesh",
                path,
            )?;
            let parent = nonempty_string(
                required_member(attachment, "parent", path)?,
                &pointer(path, "parent"),
            )?;
            let source_skin =
                optional_nonempty_string(attachment, "skin", path)?.unwrap_or("default");
            linked_meshes.push(PendingLinkedMesh {
                attachment: attachment_index,
                source_skin: source_skin.into(),
                parent: parent.into(),
                path: path.into(),
            });
            AttachmentDataKind::Mesh(MeshAttachmentData {
                colour: colour_or(attachment, "color", path, Rgba8::WHITE)?,
                atlas_region,
                geometry: u32::MAX,
                source_mesh: None,
                inherits_deform: bool_or(attachment, "deform", path, true)?,
            })
        }
        "mesh" | "linkedmesh" => {
            let reason = if sequence.is_some() {
                "an unsupported image sequence".to_owned()
            } else {
                let field = if source_type == "mesh" {
                    unknown_mesh_field
                } else {
                    unknown_linked_mesh_field
                };
                format!(
                    "unknown field {:?} with no safe fallback",
                    field.map(JsonMember::name).unwrap_or("unknown")
                )
            };
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::UnsupportedAttachmentType,
                PendingScope::Attachment(attachment_index),
                format!("{source_type} attachment {placeholder_name:?} is unsupported: {reason}"),
            ));
            AttachmentDataKind::Unsupported {
                source_type: source_type.into(),
            }
        }
        "boundingbox" => {
            pending.push(PendingDiagnostic::warning(
                DiagnosticCode::UnsupportedAttachmentType,
                PendingScope::Attachment(attachment_index),
                format!("bounding-box attachment {placeholder_name:?} is retained as metadata"),
            ));
            AttachmentDataKind::BoundingBox
        }
        "point" => {
            pending.push(PendingDiagnostic::warning(
                DiagnosticCode::UnsupportedAttachmentType,
                PendingScope::Attachment(attachment_index),
                format!("point attachment {placeholder_name:?} is retained as metadata"),
            ));
            AttachmentDataKind::Point
        }
        unsupported => {
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::UnsupportedAttachmentType,
                PendingScope::Attachment(attachment_index),
                format!("attachment {placeholder_name:?} uses unsupported type {unsupported:?}"),
            ));
            AttachmentDataKind::Unsupported {
                source_type: unsupported.into(),
            }
        }
    };

    Ok(AttachmentData {
        placeholder_name: placeholder_name.into(),
        name: actual_name.into(),
        atlas_path,
        skin,
        slot,
        kind,
    })
}

fn attachment_pixel_size(
    attachment: &[JsonMember],
    field: &str,
    path: &str,
) -> Result<u32, LoadError> {
    let field_path = pointer(path, field);
    let value = required_member(attachment, field, path)?;
    u32_value(value, &field_path)
}

fn validate_setup_attachments(
    slots: &[SlotData],
    attachment_names: &HashMap<u32, HashSet<Box<str>>>,
) -> Result<(), LoadError> {
    for (slot_index, slot) in slots.iter().enumerate() {
        let slot_index_u32 = index_u32(slot_index, "/slots")?;
        let Some(setup_name) = slot.setup_attachment_name.as_deref() else {
            continue;
        };
        if !attachment_names
            .get(&slot_index_u32)
            .is_some_and(|names| names.contains(setup_name))
        {
            return Err(error(
                LoadErrorKind::UnresolvedReference,
                &format!("/slots/{slot_index}/attachment"),
                format!("setup attachment {setup_name:?} does not exist in any skin for this slot"),
            ));
        }
    }
    Ok(())
}

type AttachmentIndexes = (
    HashMap<(u32, u32), HashMap<Box<str>, u32>>,
    HashMap<u32, HashSet<Box<str>>>,
);

fn index_attachments(attachments: &[AttachmentData]) -> Result<AttachmentIndexes, LoadError> {
    let mut by_skin_slot = HashMap::<(u32, u32), HashMap<Box<str>, u32>>::new();
    let mut names_by_slot = HashMap::<u32, HashSet<Box<str>>>::new();
    for (index, attachment) in attachments.iter().enumerate() {
        let index = index_u32(index, "/skins")?;
        by_skin_slot
            .entry((attachment.skin, attachment.slot))
            .or_default()
            .insert(attachment.placeholder_name.clone(), index);
        names_by_slot
            .entry(attachment.slot)
            .or_default()
            .insert(attachment.placeholder_name.clone());
    }
    Ok((by_skin_slot, names_by_slot))
}

type ConstraintParse = (
    Box<[ConstraintData]>,
    Box<[IkConstraintData]>,
    HashMap<Box<str>, u32>,
    Box<[TransformConstraintData]>,
    HashMap<Box<str>, u32>,
);

struct ConstraintRecord<'a> {
    value: &'a JsonValue,
    path: String,
    source_type: Option<&'static str>,
    default_order: u32,
}

fn parse_constraints(
    root: &[JsonMember],
    bones: &[BoneData],
    bone_names: &HashMap<Box<str>, u32>,
    pending: &mut PendingDiagnostics,
) -> Result<ConstraintParse, LoadError> {
    let unified = member(root, "constraints", "")?;
    let has_separate = ["ik", "transform", "path", "physics"]
        .into_iter()
        .any(|name| root.iter().any(|member| member.name() == name));
    if unified.is_some() && has_separate {
        return Err(schema_error(
            "/constraints",
            "constraints are specified in both unified and separate arrays",
        ));
    }

    let mut records = Vec::<ConstraintRecord<'_>>::new();
    if let Some(value) = unified {
        let values = array(value, "/constraints")?;
        ensure_capacity(values.len(), "/constraints")?;
        for (index, value) in values.iter().enumerate() {
            records.push(ConstraintRecord {
                value,
                path: index_pointer("/constraints", index),
                source_type: None,
                default_order: index_u32(index, "/constraints")?,
            });
        }
    } else {
        for section in root {
            let source_type = match section.name() {
                "ik" => "ik",
                "transform" => "transform",
                "path" => "path",
                "physics" => "physics",
                _ => continue,
            };
            let base = pointer("", section.name());
            let values = array(section.value(), &base)?;
            for (index, value) in values.iter().enumerate() {
                records.push(ConstraintRecord {
                    value,
                    path: index_pointer(&base, index),
                    source_type: Some(source_type),
                    default_order: 0,
                });
            }
        }
        ensure_capacity(records.len(), "/constraints")?;
    }

    let mut constraints = Vec::with_capacity(records.len());
    let mut raw_ik = Vec::<(IkConstraintData, Vec<Box<str>>)>::new();
    let mut raw_transform = Vec::<(TransformConstraintData, Vec<Box<str>>)>::new();
    let mut names = HashSet::with_capacity(records.len());
    let mut orders = HashSet::with_capacity(records.len());

    for record in records {
        let path = record.path;
        let constraint = object(record.value, &path)?;
        unique_members(constraint, &path)?;
        let name = nonempty_string(
            required_member(constraint, "name", &path)?,
            &pointer(&path, "name"),
        )?;
        if !names.insert(Box::<str>::from(name)) {
            return Err(error(
                LoadErrorKind::DuplicateName,
                &pointer(&path, "name"),
                format!("constraint name {name:?} is duplicated"),
            ));
        }
        let source_type = if let Some(source_type) = record.source_type {
            source_type
        } else {
            nonempty_string(
                required_member(constraint, "type", &path)?,
                &pointer(&path, "type"),
            )?
        };
        let constraint_index = index_u32(constraints.len(), &path)?;
        let order = u32_or(constraint, "order", &path, record.default_order)?;
        if !orders.insert(order) {
            return Err(error(
                LoadErrorKind::InvalidOrder,
                &pointer(&path, "order"),
                format!("constraint evaluation order {order} is duplicated"),
            ));
        }
        constraints.push(ConstraintData {
            name: name.into(),
            source_type: source_type.into(),
            order,
            ik_constraint: None,
            transform_constraint: None,
        });

        match source_type {
            "ik" => raw_ik.push(parse_ik_constraint(
                constraint,
                name,
                constraint_index,
                order,
                &path,
                bones,
                bone_names,
            )?),
            "transform" => raw_transform.push(parse_transform_constraint(
                constraint,
                name,
                constraint_index,
                order,
                &path,
                bones,
                bone_names,
            )?),
            _ => {
                pending.push(PendingDiagnostic::degraded(
                    DiagnosticCode::UnsupportedConstraintType,
                    PendingScope::Constraint(constraint_index),
                    format!("constraint {name:?} uses unsupported type {source_type:?}"),
                ));
            }
        }
    }

    raw_ik.sort_by_key(|(constraint, _messages)| constraint.order);
    let mut ik_constraints = Vec::with_capacity(raw_ik.len());
    let mut ik_names = HashMap::with_capacity(raw_ik.len());
    for (index, (constraint, messages)) in raw_ik.into_iter().enumerate() {
        let index = index_u32(index, "/constraints")?;
        let constraint_record = constraints
            .get_mut(constraint.constraint as usize)
            .ok_or_else(|| schema_error("/constraints", "IK constraint link is invalid"))?;
        constraint_record.ik_constraint = Some(index);
        ik_names.insert(constraint.name.clone(), index);
        for message in messages {
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::UnsupportedConstraintOption,
                PendingScope::IkConstraint(index),
                message,
            ));
        }
        ik_constraints.push(constraint);
    }

    raw_transform.sort_by_key(|(constraint, _messages)| constraint.order);
    let mut transform_constraints = Vec::with_capacity(raw_transform.len());
    let mut transform_names = HashMap::with_capacity(raw_transform.len());
    for (index, (constraint, messages)) in raw_transform.into_iter().enumerate() {
        let index = index_u32(index, "/constraints")?;
        let constraint_record = constraints
            .get_mut(constraint.constraint as usize)
            .ok_or_else(|| schema_error("/constraints", "transform constraint link is invalid"))?;
        constraint_record.transform_constraint = Some(index);
        transform_names.insert(constraint.name.clone(), index);
        for message in messages {
            pending.push(PendingDiagnostic::degraded(
                DiagnosticCode::UnsupportedConstraintOption,
                PendingScope::Constraint(constraint.constraint),
                message,
            ));
        }
        transform_constraints.push(constraint);
    }

    Ok((
        constraints.into_boxed_slice(),
        ik_constraints.into_boxed_slice(),
        ik_names,
        transform_constraints.into_boxed_slice(),
        transform_names,
    ))
}

fn parse_ik_constraint(
    constraint: &[JsonMember],
    name: &str,
    constraint_index: u32,
    order: u32,
    path: &str,
    bones: &[BoneData],
    bone_names: &HashMap<Box<str>, u32>,
) -> Result<(IkConstraintData, Vec<Box<str>>), LoadError> {
    let bone_values = array(
        required_member(constraint, "bones", path)?,
        &pointer(path, "bones"),
    )?;
    if !(1..=2).contains(&bone_values.len()) {
        return Err(error(
            LoadErrorKind::InvalidTopology,
            &pointer(path, "bones"),
            "IK constraints require exactly one or two bones",
        ));
    }
    let mut constrained = Vec::with_capacity(bone_values.len());
    for (index, value) in bone_values.iter().enumerate() {
        let bone_path = index_pointer(&pointer(path, "bones"), index);
        let bone_name = nonempty_string(value, &bone_path)?;
        constrained.push(*bone_names.get(bone_name).ok_or_else(|| {
            error(
                LoadErrorKind::UnresolvedReference,
                &bone_path,
                format!("IK bone {bone_name:?} does not exist"),
            )
        })?);
    }
    if let [parent, child] = constrained.as_slice() {
        let child_parent = bones.get(*child as usize).and_then(|bone| bone.parent);
        if child_parent != Some(*parent) {
            return Err(error(
                LoadErrorKind::InvalidTopology,
                &pointer(path, "bones"),
                "a two-bone IK chain must list a parent followed by its direct child",
            ));
        }
    }
    let target_name = nonempty_string(
        required_member(constraint, "target", path)?,
        &pointer(path, "target"),
    )?;
    let target = bone_names.get(target_name).copied().ok_or_else(|| {
        error(
            LoadErrorKind::UnresolvedReference,
            &pointer(path, "target"),
            format!("IK target bone {target_name:?} does not exist"),
        )
    })?;
    let mut ancestor = Some(target);
    while let Some(index) = ancestor {
        if constrained.contains(&index) {
            return Err(error(
                LoadErrorKind::InvalidTopology,
                &pointer(path, "target"),
                "IK target must not be a constrained bone or one of its descendants",
            ));
        }
        ancestor = bones.get(index as usize).and_then(|bone| bone.parent);
    }
    let mix_value = f32_or(constraint, "mix", path, 1.0)?;
    let mix = Mix::new(mix_value).map_err(|_error| {
        schema_error(
            &pointer(path, "mix"),
            "IK mix must be in the inclusive range 0 through 1",
        )
    })?;
    let softness = f32_or(constraint, "softness", path, 0.0)?;
    if softness < 0.0 {
        return Err(schema_error(
            &pointer(path, "softness"),
            "IK softness must be nonnegative",
        ));
    }
    let compress = bool_or(constraint, "compress", path, false)?;
    let stretch = bool_or(constraint, "stretch", path, false)?;
    let uniform = bool_or(constraint, "uniform", path, false)?;
    let skin = bool_or(constraint, "skin", path, false)?;
    let mut unsupported = Vec::new();
    if softness != 0.0 {
        unsupported.push(format!("IK constraint {name:?} uses unsupported softness").into());
    }
    if compress {
        unsupported.push(format!("IK constraint {name:?} enables unsupported compression").into());
    }
    if stretch {
        unsupported.push(format!("IK constraint {name:?} enables unsupported stretching").into());
    }
    if uniform {
        unsupported
            .push(format!("IK constraint {name:?} enables unsupported uniform scaling").into());
    }
    if skin {
        unsupported.push(format!("IK constraint {name:?} is skin-specific").into());
    }
    for field in constraint {
        if !matches!(
            field.name(),
            "type"
                | "name"
                | "order"
                | "bones"
                | "target"
                | "mix"
                | "softness"
                | "bendPositive"
                | "compress"
                | "stretch"
                | "uniform"
                | "skin"
        ) {
            unsupported.push(
                format!(
                    "IK constraint {name:?} contains unknown option {:?}",
                    field.name()
                )
                .into(),
            );
        }
    }

    Ok((
        IkConstraintData {
            constraint: constraint_index,
            name: name.into(),
            order,
            bones: constrained.into_boxed_slice(),
            target,
            mix,
            bend_direction: if bool_or(constraint, "bendPositive", path, true)? {
                BendDirection::Positive
            } else {
                BendDirection::Negative
            },
            softness,
            compress,
            stretch,
            uniform,
        },
        unsupported,
    ))
}

fn parse_transform_constraint(
    constraint: &[JsonMember],
    name: &str,
    constraint_index: u32,
    order: u32,
    path: &str,
    bones: &[BoneData],
    bone_names: &HashMap<Box<str>, u32>,
) -> Result<(TransformConstraintData, Vec<Box<str>>), LoadError> {
    let bone_values = array(
        required_member(constraint, "bones", path)?,
        &pointer(path, "bones"),
    )?;
    if bone_values.is_empty() {
        return Err(error(
            LoadErrorKind::InvalidTopology,
            &pointer(path, "bones"),
            "transform constraints require at least one constrained bone",
        ));
    }
    let mut constrained = Vec::with_capacity(bone_values.len());
    let mut seen_bones = HashSet::with_capacity(bone_values.len());
    for (index, value) in bone_values.iter().enumerate() {
        let bone_path = index_pointer(&pointer(path, "bones"), index);
        let bone_name = nonempty_string(value, &bone_path)?;
        let bone = *bone_names.get(bone_name).ok_or_else(|| {
            error(
                LoadErrorKind::UnresolvedReference,
                &bone_path,
                format!("transform constraint bone {bone_name:?} does not exist"),
            )
        })?;
        if !seen_bones.insert(bone) {
            return Err(error(
                LoadErrorKind::InvalidTopology,
                &bone_path,
                format!("transform constraint bone {bone_name:?} is listed more than once"),
            ));
        }
        constrained.push(bone);
    }

    let source_name = aliased_nonempty_string(constraint, "source", "target", path)?;
    let source = bone_names.get(source_name).copied().ok_or_else(|| {
        error(
            LoadErrorKind::UnresolvedReference,
            &pointer(path, "source"),
            format!("transform constraint source bone {source_name:?} does not exist"),
        )
    })?;
    let mut ancestor = Some(source);
    while let Some(index) = ancestor {
        if constrained.contains(&index) {
            return Err(error(
                LoadErrorKind::InvalidTopology,
                &pointer(path, "source"),
                "transform constraint source must not be a constrained bone or its descendant",
            ));
        }
        ancestor = bones.get(index as usize).and_then(|bone| bone.parent);
    }

    let mut unsupported = Vec::<Box<str>>::new();
    let legacy_property_map = member(constraint, "properties", path)?.is_none();
    let properties = parse_transform_properties(constraint, name, path, &mut unsupported)?;
    let (local_source, local_target) = transform_local_modes(constraint, path)?;
    let additive = aliased_bool(constraint, "additive", "relative", path, false)?;
    let clamped = bool_or(constraint, "clamp", path, false)?;
    let skin = bool_or(constraint, "skin", path, false)?;
    if local_source {
        unsupported.push(
            format!("transform constraint {name:?} reads unsupported local source values").into(),
        );
    }
    if local_target {
        unsupported.push(
            format!("transform constraint {name:?} writes unsupported local target values").into(),
        );
    }
    if additive {
        unsupported
            .push(format!("transform constraint {name:?} uses unsupported additive values").into());
    }
    if clamped {
        unsupported.push(
            format!("transform constraint {name:?} enables unsupported property clamping").into(),
        );
    }
    if skin {
        unsupported.push(format!("transform constraint {name:?} is skin-specific").into());
    }

    let setup_pose = TransformConstraintPoseData {
        mix_rotate: transform_mix(
            constraint,
            "mixRotate",
            path,
            f32::from(properties.rotation),
        )?,
        mix_x: transform_mix(constraint, "mixX", path, f32::from(properties.x))?,
        mix_y: transform_mix_with_fallback(
            constraint,
            "mixY",
            "mixX",
            path,
            f32::from(properties.y),
        )?,
        mix_scale_x: transform_mix(constraint, "mixScaleX", path, f32::from(properties.scale_x))?,
        mix_scale_y: transform_mix_with_fallback(
            constraint,
            "mixScaleY",
            "mixScaleX",
            path,
            f32::from(properties.scale_y),
        )?,
        mix_shear_y: transform_mix(constraint, "mixShearY", path, f32::from(properties.shear_y))?,
    };
    let offsets = [
        ("X translation", f32_or(constraint, "x", path, 0.0)?),
        ("Y translation", f32_or(constraint, "y", path, 0.0)?),
        ("X scale", f32_or(constraint, "scaleX", path, 0.0)?),
        ("Y scale", f32_or(constraint, "scaleY", path, 0.0)?),
        ("Y shear", f32_or(constraint, "shearY", path, 0.0)?),
    ];
    if legacy_property_map {
        for (field, mix) in [
            ("X translation", setup_pose.mix_x),
            ("Y translation", setup_pose.mix_y),
            ("X scale", setup_pose.mix_scale_x),
            ("Y scale", setup_pose.mix_scale_y),
            ("Y shear", setup_pose.mix_shear_y),
        ] {
            if mix != TransformMix::ZERO {
                unsupported.push(
                    format!("transform constraint {name:?} has unsupported {field} influence")
                        .into(),
                );
            }
        }
        for (field, value) in offsets {
            if value != 0.0 {
                unsupported.push(
                    format!("transform constraint {name:?} uses unsupported {field} offset").into(),
                );
            }
        }
    }

    for field in constraint {
        if !matches!(
            field.name(),
            "type"
                | "name"
                | "order"
                | "bones"
                | "source"
                | "target"
                | "rotation"
                | "x"
                | "y"
                | "scaleX"
                | "scaleY"
                | "shearY"
                | "mixRotate"
                | "mixX"
                | "mixY"
                | "mixScaleX"
                | "mixScaleY"
                | "mixShearY"
                | "localSource"
                | "localTarget"
                | "local"
                | "additive"
                | "relative"
                | "clamp"
                | "skin"
                | "properties"
        ) {
            unsupported.push(
                format!(
                    "transform constraint {name:?} contains unknown option {:?}",
                    field.name()
                )
                .into(),
            );
        }
    }

    let rotation_offset = Angle::from_degrees(f32_or(constraint, "rotation", path, 0.0)?)
        .expect("the JSON schema rejects non-finite transform constraint offsets");
    Ok((
        TransformConstraintData {
            constraint: constraint_index,
            name: name.into(),
            order,
            bones: constrained.into_boxed_slice(),
            source,
            rotation_offset,
            copies_rotation: properties.direct_rotation && properties.direct_rotation_supported,
            local_source,
            local_target,
            additive,
            clamped,
            setup_pose,
        },
        unsupported,
    ))
}

#[derive(Clone, Copy)]
struct TransformPropertyMap {
    rotation: bool,
    x: bool,
    y: bool,
    scale_x: bool,
    scale_y: bool,
    shear_y: bool,
    direct_rotation: bool,
    direct_rotation_supported: bool,
}

impl TransformPropertyMap {
    const LEGACY: Self = Self {
        rotation: true,
        x: true,
        y: true,
        scale_x: true,
        scale_y: true,
        shear_y: true,
        direct_rotation: true,
        direct_rotation_supported: true,
    };

    const EMPTY: Self = Self {
        rotation: false,
        x: false,
        y: false,
        scale_x: false,
        scale_y: false,
        shear_y: false,
        direct_rotation: false,
        direct_rotation_supported: true,
    };

    fn mark(&mut self, property: &str) {
        match property {
            "rotate" => self.rotation = true,
            "x" => self.x = true,
            "y" => self.y = true,
            "scaleX" => self.scale_x = true,
            "scaleY" => self.scale_y = true,
            "shearY" => self.shear_y = true,
            _ => {}
        }
    }
}

fn parse_transform_properties(
    constraint: &[JsonMember],
    name: &str,
    path: &str,
    unsupported: &mut Vec<Box<str>>,
) -> Result<TransformPropertyMap, LoadError> {
    let Some(value) = member(constraint, "properties", path)? else {
        // The pre-4.3 transform format implicitly copied like-named transform
        // channels and is retained for compatible historical fixtures.
        return Ok(TransformPropertyMap::LEGACY);
    };
    let properties_path = pointer(path, "properties");
    let properties = object(value, &properties_path)?;
    unique_members(properties, &properties_path)?;
    let mut mapping = TransformPropertyMap::EMPTY;
    for property in properties {
        let property_path = pointer(&properties_path, property.name());
        let from = object(property.value(), &property_path)?;
        unique_members(from, &property_path)?;
        let to_path = pointer(&property_path, "to");
        let to = object(required_member(from, "to", &property_path)?, &to_path)?;
        unique_members(to, &to_path)?;
        for destination in to {
            let destination_path = pointer(&to_path, destination.name());
            let settings = object(destination.value(), &destination_path)?;
            unique_members(settings, &destination_path)?;
            mapping.mark(destination.name());
            if property.name() == "rotate" && destination.name() == "rotate" {
                mapping.direct_rotation = true;
                for setting in settings {
                    if setting.name() == "max" {
                        let _max = finite_f32(setting.value(), &pointer(&destination_path, "max"))?;
                    } else {
                        mapping.direct_rotation_supported = false;
                        unsupported.push(
                            format!(
                                "transform constraint {name:?} uses unsupported rotation mapping option {:?}",
                                setting.name()
                            )
                            .into(),
                        );
                    }
                }
            } else {
                if destination.name() == "rotate" {
                    mapping.direct_rotation_supported = false;
                }
                unsupported.push(
                    format!(
                        "transform constraint {name:?} maps unsupported property {:?} to {:?}",
                        property.name(),
                        destination.name()
                    )
                    .into(),
                );
            }
        }
        for field in from {
            if field.name() != "to" {
                if property.name() == "rotate" {
                    mapping.direct_rotation_supported = false;
                }
                unsupported.push(
                    format!(
                        "transform constraint {name:?} contains unsupported source-property option {:?}",
                        field.name()
                    )
                    .into(),
                );
            }
        }
    }
    Ok(mapping)
}

fn transform_local_modes(object: &[JsonMember], path: &str) -> Result<(bool, bool), LoadError> {
    let local_source = member(object, "localSource", path)?;
    let local_target = member(object, "localTarget", path)?;
    let legacy = member(object, "local", path)?;
    if legacy.is_some() && (local_source.is_some() || local_target.is_some()) {
        return Err(schema_error(
            path,
            "constraint local mode is specified by both modern and legacy fields",
        ));
    }
    if let Some(value) = legacy {
        let local = value
            .as_bool()
            .ok_or_else(|| schema_error(&pointer(path, "local"), "value must be a boolean"))?;
        return Ok((local, local));
    }
    let parse = |value: Option<&JsonValue>, field: &str| {
        value.map_or(Ok(false), |value| {
            value
                .as_bool()
                .ok_or_else(|| schema_error(&pointer(path, field), "value must be a boolean"))
        })
    };
    Ok((
        parse(local_source, "localSource")?,
        parse(local_target, "localTarget")?,
    ))
}

fn aliased_nonempty_string<'a>(
    object: &'a [JsonMember],
    modern: &str,
    legacy: &str,
    path: &str,
) -> Result<&'a str, LoadError> {
    let modern_value = member(object, modern, path)?;
    let legacy_value = member(object, legacy, path)?;
    match (modern_value, legacy_value) {
        (Some(_), Some(_)) => Err(schema_error(
            path,
            format!("constraint source is specified by both {modern:?} and {legacy:?}"),
        )),
        (Some(value), None) => nonempty_string(value, &pointer(path, modern)),
        (None, Some(value)) => nonempty_string(value, &pointer(path, legacy)),
        (None, None) => Err(schema_error(
            &pointer(path, modern),
            "transform constraint source bone is required",
        )),
    }
}

fn aliased_bool(
    object: &[JsonMember],
    modern: &str,
    legacy: &str,
    path: &str,
    default: bool,
) -> Result<bool, LoadError> {
    let modern_value = member(object, modern, path)?;
    let legacy_value = member(object, legacy, path)?;
    match (modern_value, legacy_value) {
        (Some(_), Some(_)) => Err(schema_error(
            path,
            format!("constraint option is specified by both {modern:?} and {legacy:?}"),
        )),
        (Some(value), None) => value
            .as_bool()
            .ok_or_else(|| schema_error(&pointer(path, modern), "value must be a boolean")),
        (None, Some(value)) => value
            .as_bool()
            .ok_or_else(|| schema_error(&pointer(path, legacy), "value must be a boolean")),
        (None, None) => Ok(default),
    }
}

fn transform_mix(
    object: &[JsonMember],
    field: &str,
    path: &str,
    default: f32,
) -> Result<TransformMix, LoadError> {
    TransformMix::new(f32_or(object, field, path, default)?).map_err(|_error| {
        schema_error(
            &pointer(path, field),
            "transform constraint mix must be finite",
        )
    })
}

fn transform_mix_with_fallback(
    object: &[JsonMember],
    field: &str,
    fallback: &str,
    path: &str,
    default: f32,
) -> Result<TransformMix, LoadError> {
    let value = match member(object, field, path)? {
        Some(value) => finite_f32(value, &pointer(path, field))?,
        None => match member(object, fallback, path)? {
            Some(value) => finite_f32(value, &pointer(path, fallback))?,
            None => default,
        },
    };
    TransformMix::new(value).map_err(|_error| {
        schema_error(
            &pointer(path, field),
            "transform constraint mix must be finite",
        )
    })
}

type EventParse = (Box<[EventDefinitionData]>, HashMap<Box<str>, u32>);

fn parse_events(
    root: &[JsonMember],
    pending: &mut PendingDiagnostics,
) -> Result<EventParse, LoadError> {
    let Some(value) = member(root, "events", "")? else {
        return Ok((Box::default(), HashMap::new()));
    };
    let events = object(value, "/events")?;
    unique_members(events, "/events")?;
    ensure_capacity(events.len(), "/events")?;
    let mut definitions = Vec::with_capacity(events.len());
    let mut names = HashMap::with_capacity(events.len());
    for (index, event) in events.iter().enumerate() {
        let path = pointer("/events", event.name());
        if event.name().is_empty() {
            return Err(schema_error(&path, "event name must not be empty"));
        }
        let data = object(event.value(), &path)?;
        unique_members(data, &path)?;
        let event_index = index_u32(index, &path)?;
        let integer = match member(data, "int", &path)? {
            None => 0,
            Some(value) => i32_value(value, &pointer(&path, "int"))?,
        };
        let payload = EventPayload {
            integer,
            float: f32_or(data, "float", &path, 0.0)?,
            string: optional_string(data, "string", &path)?.map(Box::from),
            volume: f32_or(data, "volume", &path, 1.0)?,
            balance: f32_or(data, "balance", &path, 0.0)?,
        };
        for field in data {
            if !matches!(
                field.name(),
                "int" | "float" | "string" | "audio" | "volume" | "balance"
            ) {
                pending.push(PendingDiagnostic::degraded(
                    DiagnosticCode::UnknownField,
                    PendingScope::Event(event_index),
                    format!(
                        "event {:?} contains unknown field {:?}",
                        event.name(),
                        field.name()
                    ),
                ));
            }
        }
        names.insert(Box::from(event.name()), event_index);
        definitions.push(EventDefinitionData {
            name: event.name().into(),
            payload,
            audio: optional_string(data, "audio", &path)?.map(Box::from),
        });
    }
    Ok((definitions.into_boxed_slice(), names))
}

fn ensure_capacity(length: usize, path: &str) -> Result<(), LoadError> {
    if u32::try_from(length).is_ok() {
        Ok(())
    } else {
        Err(error(
            LoadErrorKind::CapacityExceeded,
            path,
            "table contains more entries than asset-scoped IDs can represent",
        ))
    }
}

fn index_u32(index: usize, path: &str) -> Result<u32, LoadError> {
    u32::try_from(index).map_err(|_error| {
        error(
            LoadErrorKind::CapacityExceeded,
            path,
            "table index exceeds the asset-scoped ID representation",
        )
    })
}

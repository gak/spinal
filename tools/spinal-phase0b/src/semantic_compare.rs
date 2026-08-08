//! Deterministic comparison of complete Spinal semantic frames.
//!
//! The tolerances in this module are the fixed Phase 0B version 1 policy. They
//! are deliberately not caller-configurable. Structural fields are compared
//! exactly and numeric fields use the tolerance assigned to their semantic
//! category.

use std::fmt::{self, Debug, Write};

use serde::Serialize;
use spinal::{
    SemanticAtlasRegion, SemanticAttachment, SemanticBone, SemanticDiagnostic,
    SemanticDiagnosticScope, SemanticDraw, SemanticFrame, SemanticIkConstraint, SemanticSlot,
    SemanticTransformConstraint,
};
use thiserror::Error;

use crate::contract::{ANGLE_RADIANS_ABS, AXIS_ABS, COLOR_ABS, POSITION_ABS, UNITLESS_ABS, UV_ABS};

/// The structured semantic-comparison report schema emitted by this module.
pub const SEMANTIC_COMPARISON_FORMAT_VERSION: u16 = 1;

/// The maximum number of detailed differences retained in one report.
pub const MAX_SEMANTIC_DIFFERENCES: usize = 256;

/// The maximum UTF-8 byte length of a difference path.
pub const MAX_SEMANTIC_DIFFERENCE_PATH_BYTES: usize = 192;

/// The maximum UTF-8 byte length of either rendered value in a difference.
pub const MAX_SEMANTIC_DIFFERENCE_VALUE_BYTES: usize = 256;

/// A deterministic, bounded comparison of two complete semantic frames.
///
/// The absence of retained and omitted differences means the frames agree
/// under the fixed comparison policy. This report deliberately carries no
/// gate, approval, or rehearsal status.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SemanticComparison {
    format_version: u16,
    differences: Vec<SemanticDifference>,
    omitted_difference_count: usize,
}

impl SemanticComparison {
    /// Returns the comparison-report schema version.
    #[must_use]
    pub const fn format_version(&self) -> u16 {
        self.format_version
    }

    /// Returns retained differences in deterministic traversal order.
    #[must_use]
    pub fn differences(&self) -> &[SemanticDifference] {
        &self.differences
    }

    /// Returns how many differences were omitted after the fixed report cap.
    #[must_use]
    pub const fn omitted_difference_count(&self) -> usize {
        self.omitted_difference_count
    }
}

/// One exact or out-of-tolerance semantic difference.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SemanticDifference {
    path: Box<str>,
    expected: Box<str>,
    actual: Box<str>,
}

impl SemanticDifference {
    /// Returns the stable, index-based semantic field path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the bounded deterministic rendering of the reference value.
    #[must_use]
    pub fn expected(&self) -> &str {
        &self.expected
    }

    /// Returns the bounded deterministic rendering of the observed value.
    #[must_use]
    pub fn actual(&self) -> &str {
        &self.actual
    }
}

/// A semantic-frame JSON input was rejected before comparison.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum SemanticCompareError {
    /// The expected/reference frame is not strict valid semantic-frame JSON.
    #[error("expected semantic frame is invalid: {message}")]
    InvalidExpectedFrame {
        /// Bounded parser detail.
        message: Box<str>,
    },
    /// The actual/observed frame is not strict valid semantic-frame JSON.
    #[error("actual semantic frame is invalid: {message}")]
    InvalidActualFrame {
        /// Bounded parser detail.
        message: Box<str>,
    },
}

/// Parses two strict semantic-frame JSON documents and compares them.
///
/// Parsing is performed by [`SemanticFrame::from_json`], which rejects unknown
/// fields, unsupported schema versions, non-finite numbers, invalid colors,
/// and malformed geometry before this comparator can inspect the frames.
pub fn compare_semantic_frame_json(
    expected_json: &[u8],
    actual_json: &[u8],
) -> Result<SemanticComparison, SemanticCompareError> {
    let expected = SemanticFrame::from_json(expected_json).map_err(|error| {
        SemanticCompareError::InvalidExpectedFrame {
            message: bounded_debug(&error, MAX_SEMANTIC_DIFFERENCE_VALUE_BYTES),
        }
    })?;
    let actual = SemanticFrame::from_json(actual_json).map_err(|error| {
        SemanticCompareError::InvalidActualFrame {
            message: bounded_debug(&error, MAX_SEMANTIC_DIFFERENCE_VALUE_BYTES),
        }
    })?;
    Ok(compare_semantic_frames(&expected, &actual))
}

/// Compares two already validated complete semantic frames.
///
/// Callers reading evidence from disk should normally use
/// [`compare_semantic_frame_json`] so validation and comparison remain one
/// operation.
#[must_use]
pub fn compare_semantic_frames(
    expected: &SemanticFrame,
    actual: &SemanticFrame,
) -> SemanticComparison {
    let mut differences = Differences::default();

    differences.exact(
        "format_version",
        &expected.format_version(),
        &actual.format_version(),
    );
    differences.exact(
        "default_skin",
        &expected.default_skin(),
        &actual.default_skin(),
    );
    compare_skin_layers(&mut differences, expected, actual);
    compare_bones(&mut differences, expected.bones(), actual.bones());
    compare_slots(&mut differences, expected.slots(), actual.slots());
    compare_draw_items(&mut differences, expected.draw_items(), actual.draw_items());
    compare_ik_constraints(
        &mut differences,
        expected.ik_constraints(),
        actual.ik_constraints(),
    );
    compare_transform_constraints(
        &mut differences,
        expected.transform_constraints(),
        actual.transform_constraints(),
    );
    compare_diagnostics(
        &mut differences,
        expected.active_diagnostics(),
        actual.active_diagnostics(),
    );

    SemanticComparison {
        format_version: SEMANTIC_COMPARISON_FORMAT_VERSION,
        differences: differences.retained,
        omitted_difference_count: differences.omitted,
    }
}

#[derive(Default)]
struct Differences {
    retained: Vec<SemanticDifference>,
    omitted: usize,
}

impl Differences {
    fn exact<T>(&mut self, path: &str, expected: &T, actual: &T)
    where
        T: Debug + PartialEq + ?Sized,
    {
        if expected != actual {
            self.record(path, expected, actual);
        }
    }

    fn numeric(&mut self, path: &str, expected: f32, actual: f32, absolute_tolerance: f64) {
        let delta = (f64::from(expected) - f64::from(actual)).abs();
        if delta > absolute_tolerance {
            self.record(path, &expected, &actual);
        }
    }

    fn record<T, U>(&mut self, path: &str, expected: &T, actual: &U)
    where
        T: Debug + ?Sized,
        U: Debug + ?Sized,
    {
        if self.retained.len() == MAX_SEMANTIC_DIFFERENCES {
            self.omitted = self.omitted.saturating_add(1);
            return;
        }
        self.retained.push(SemanticDifference {
            path: bounded_plain(path, MAX_SEMANTIC_DIFFERENCE_PATH_BYTES),
            expected: bounded_debug(expected, MAX_SEMANTIC_DIFFERENCE_VALUE_BYTES),
            actual: bounded_debug(actual, MAX_SEMANTIC_DIFFERENCE_VALUE_BYTES),
        });
    }

    fn length(&mut self, path: &str, expected: usize, actual: usize) {
        self.exact(&format!("{path}.length"), &expected, &actual);
    }
}

fn compare_skin_layers(
    differences: &mut Differences,
    expected: &SemanticFrame,
    actual: &SemanticFrame,
) {
    let expected_layers = expected.skin_layers();
    let actual_layers = actual.skin_layers();
    differences.length("skin_layers", expected_layers.len(), actual_layers.len());
    for (index, (expected_layer, actual_layer)) in expected_layers.zip(actual_layers).enumerate() {
        differences.exact(
            &format!("skin_layers[{index}]"),
            expected_layer,
            actual_layer,
        );
    }
}

fn compare_bones(
    differences: &mut Differences,
    expected: &[SemanticBone],
    actual: &[SemanticBone],
) {
    differences.length("bones", expected.len(), actual.len());
    for (index, (expected_bone, actual_bone)) in expected.iter().zip(actual).enumerate() {
        let path = format!("bones[{index}]");
        differences.exact(
            &format!("{path}.ordinal"),
            &expected_bone.ordinal(),
            &actual_bone.ordinal(),
        );
        differences.exact(
            &format!("{path}.name"),
            expected_bone.name(),
            actual_bone.name(),
        );

        let expected_local = expected_bone.local();
        let actual_local = actual_bone.local();
        compare_pair(
            differences,
            &format!("{path}.local.translation"),
            expected_local.translation(),
            actual_local.translation(),
            POSITION_ABS,
        );
        differences.numeric(
            &format!("{path}.local.rotation_radians"),
            expected_local.rotation_radians(),
            actual_local.rotation_radians(),
            ANGLE_RADIANS_ABS,
        );
        compare_pair(
            differences,
            &format!("{path}.local.scale"),
            expected_local.scale(),
            actual_local.scale(),
            UNITLESS_ABS,
        );
        compare_pair(
            differences,
            &format!("{path}.local.shear_radians"),
            expected_local.shear_radians(),
            actual_local.shear_radians(),
            ANGLE_RADIANS_ABS,
        );

        let expected_world = expected_bone.world();
        let actual_world = actual_bone.world();
        compare_pair(
            differences,
            &format!("{path}.world.translation"),
            expected_world.translation(),
            actual_world.translation(),
            POSITION_ABS,
        );
        compare_pair(
            differences,
            &format!("{path}.world.x_axis"),
            expected_world.x_axis(),
            actual_world.x_axis(),
            AXIS_ABS,
        );
        compare_pair(
            differences,
            &format!("{path}.world.y_axis"),
            expected_world.y_axis(),
            actual_world.y_axis(),
            AXIS_ABS,
        );
    }
}

fn compare_slots(
    differences: &mut Differences,
    expected: &[SemanticSlot],
    actual: &[SemanticSlot],
) {
    differences.length("slots", expected.len(), actual.len());
    for (index, (expected_slot, actual_slot)) in expected.iter().zip(actual).enumerate() {
        let path = format!("slots[{index}]");
        differences.exact(
            &format!("{path}.draw_order"),
            &expected_slot.draw_order(),
            &actual_slot.draw_order(),
        );
        differences.exact(
            &format!("{path}.name"),
            expected_slot.name(),
            actual_slot.name(),
        );
        compare_optional_attachment(
            differences,
            &format!("{path}.attachment"),
            expected_slot.attachment(),
            actual_slot.attachment(),
        );
        compare_quad(
            differences,
            &format!("{path}.color_rgba"),
            expected_slot.color_rgba(),
            actual_slot.color_rgba(),
            COLOR_ABS,
        );
    }
}

fn compare_draw_items(
    differences: &mut Differences,
    expected: &[SemanticDraw],
    actual: &[SemanticDraw],
) {
    differences.length("draw_items", expected.len(), actual.len());
    for (index, (expected_draw, actual_draw)) in expected.iter().zip(actual).enumerate() {
        let path = format!("draw_items[{index}]");
        differences.exact(
            &format!("{path}.kind"),
            &expected_draw.kind(),
            &actual_draw.kind(),
        );
        differences.exact(
            &format!("{path}.slot"),
            expected_draw.slot(),
            actual_draw.slot(),
        );
        compare_attachment(
            differences,
            &format!("{path}.attachment"),
            expected_draw.attachment(),
            actual_draw.attachment(),
        );
        compare_atlas_region(
            differences,
            &format!("{path}.atlas_region"),
            expected_draw.atlas_region(),
            actual_draw.atlas_region(),
        );
        differences.exact(
            &format!("{path}.blend_mode"),
            &expected_draw.blend_mode(),
            &actual_draw.blend_mode(),
        );
        compare_vec2_slice(
            differences,
            &format!("{path}.positions"),
            expected_draw.positions(),
            actual_draw.positions(),
            POSITION_ABS,
        );
        compare_optional_vec2_slice(
            differences,
            &format!("{path}.uvs"),
            expected_draw.uvs(),
            actual_draw.uvs(),
            UV_ABS,
        );
        compare_u32_slice(
            differences,
            &format!("{path}.triangles"),
            expected_draw.triangles(),
            actual_draw.triangles(),
        );
        compare_quad(
            differences,
            &format!("{path}.color_rgba"),
            expected_draw.color_rgba(),
            actual_draw.color_rgba(),
            COLOR_ABS,
        );
    }
}

fn compare_ik_constraints(
    differences: &mut Differences,
    expected: &[SemanticIkConstraint],
    actual: &[SemanticIkConstraint],
) {
    differences.length("ik_constraints", expected.len(), actual.len());
    for (index, (expected_constraint, actual_constraint)) in expected.iter().zip(actual).enumerate()
    {
        let path = format!("ik_constraints[{index}]");
        differences.exact(
            &format!("{path}.name"),
            expected_constraint.name(),
            actual_constraint.name(),
        );
        differences.exact(
            &format!("{path}.active"),
            &expected_constraint.is_active(),
            &actual_constraint.is_active(),
        );
        differences.exact(
            &format!("{path}.preserved_underdetermined"),
            &expected_constraint.preserved_underdetermined(),
            &actual_constraint.preserved_underdetermined(),
        );
        differences.exact(
            &format!("{path}.target_reach"),
            &expected_constraint.target_reach(),
            &actual_constraint.target_reach(),
        );
        differences.exact(
            &format!("{path}.child_translation_y_zeroed"),
            &expected_constraint.child_translation_y_was_zeroed(),
            &actual_constraint.child_translation_y_was_zeroed(),
        );
        differences.exact(
            &format!("{path}.issue"),
            &expected_constraint.issue(),
            &actual_constraint.issue(),
        );
    }
}

fn compare_transform_constraints(
    differences: &mut Differences,
    expected: &[SemanticTransformConstraint],
    actual: &[SemanticTransformConstraint],
) {
    differences.length("transform_constraints", expected.len(), actual.len());
    for (index, (expected_constraint, actual_constraint)) in expected.iter().zip(actual).enumerate()
    {
        let path = format!("transform_constraints[{index}]");
        differences.exact(
            &format!("{path}.name"),
            expected_constraint.name(),
            actual_constraint.name(),
        );
        differences.exact(
            &format!("{path}.active"),
            &expected_constraint.is_active(),
            &actual_constraint.is_active(),
        );
        differences.exact(
            &format!("{path}.issue"),
            &expected_constraint.issue(),
            &actual_constraint.issue(),
        );
    }
}

fn compare_diagnostics(
    differences: &mut Differences,
    expected: &[SemanticDiagnostic],
    actual: &[SemanticDiagnostic],
) {
    differences.length("active_diagnostics", expected.len(), actual.len());
    for (index, (expected_diagnostic, actual_diagnostic)) in expected.iter().zip(actual).enumerate()
    {
        let path = format!("active_diagnostics[{index}]");
        differences.exact(
            &format!("{path}.severity"),
            &expected_diagnostic.severity(),
            &actual_diagnostic.severity(),
        );
        differences.exact(
            &format!("{path}.code"),
            &expected_diagnostic.code(),
            &actual_diagnostic.code(),
        );
        compare_diagnostic_scope(
            differences,
            &format!("{path}.scope"),
            expected_diagnostic.scope(),
            actual_diagnostic.scope(),
        );
        differences.exact(
            &format!("{path}.message"),
            expected_diagnostic.message(),
            actual_diagnostic.message(),
        );
    }
}

fn compare_diagnostic_scope(
    differences: &mut Differences,
    path: &str,
    expected: &SemanticDiagnosticScope,
    actual: &SemanticDiagnosticScope,
) {
    let expected_kind = diagnostic_scope_kind(expected);
    let actual_kind = diagnostic_scope_kind(actual);
    differences.exact(&format!("{path}.kind"), &expected_kind, &actual_kind);
    if expected_kind != actual_kind {
        return;
    }

    match (expected, actual) {
        (SemanticDiagnosticScope::Asset, SemanticDiagnosticScope::Asset) => {}
        (SemanticDiagnosticScope::Bone(expected), SemanticDiagnosticScope::Bone(actual))
        | (SemanticDiagnosticScope::Slot(expected), SemanticDiagnosticScope::Slot(actual))
        | (SemanticDiagnosticScope::Skin(expected), SemanticDiagnosticScope::Skin(actual))
        | (
            SemanticDiagnosticScope::Animation(expected),
            SemanticDiagnosticScope::Animation(actual),
        )
        | (SemanticDiagnosticScope::Event(expected), SemanticDiagnosticScope::Event(actual))
        | (
            SemanticDiagnosticScope::IkConstraint(expected),
            SemanticDiagnosticScope::IkConstraint(actual),
        )
        | (
            SemanticDiagnosticScope::Constraint(expected),
            SemanticDiagnosticScope::Constraint(actual),
        )
        | (
            SemanticDiagnosticScope::AtlasPage(expected),
            SemanticDiagnosticScope::AtlasPage(actual),
        ) => differences.exact(&format!("{path}.value"), expected.as_ref(), actual.as_ref()),
        (
            SemanticDiagnosticScope::Attachment(expected),
            SemanticDiagnosticScope::Attachment(actual),
        ) => compare_attachment(differences, &format!("{path}.value"), expected, actual),
        (
            SemanticDiagnosticScope::AtlasRegion(expected),
            SemanticDiagnosticScope::AtlasRegion(actual),
        ) => compare_atlas_region(differences, &format!("{path}.value"), expected, actual),
        _ => differences.exact(path, expected, actual),
    }
}

fn diagnostic_scope_kind(scope: &SemanticDiagnosticScope) -> &'static str {
    match scope {
        SemanticDiagnosticScope::Asset => "asset",
        SemanticDiagnosticScope::Bone(_) => "bone",
        SemanticDiagnosticScope::Slot(_) => "slot",
        SemanticDiagnosticScope::Skin(_) => "skin",
        SemanticDiagnosticScope::Animation(_) => "animation",
        SemanticDiagnosticScope::Event(_) => "event",
        SemanticDiagnosticScope::Attachment(_) => "attachment",
        SemanticDiagnosticScope::IkConstraint(_) => "ik_constraint",
        SemanticDiagnosticScope::Constraint(_) => "constraint",
        SemanticDiagnosticScope::AtlasPage(_) => "atlas_page",
        SemanticDiagnosticScope::AtlasRegion(_) => "atlas_region",
        _ => "unknown",
    }
}

fn compare_optional_attachment(
    differences: &mut Differences,
    path: &str,
    expected: Option<&SemanticAttachment>,
    actual: Option<&SemanticAttachment>,
) {
    match (expected, actual) {
        (Some(expected), Some(actual)) => {
            compare_attachment(differences, path, expected, actual);
        }
        (None, None) => {}
        _ => differences.exact(path, &expected, &actual),
    }
}

fn compare_attachment(
    differences: &mut Differences,
    path: &str,
    expected: &SemanticAttachment,
    actual: &SemanticAttachment,
) {
    differences.exact(&format!("{path}.skin"), expected.skin(), actual.skin());
    differences.exact(&format!("{path}.slot"), expected.slot(), actual.slot());
    differences.exact(
        &format!("{path}.placeholder"),
        expected.placeholder(),
        actual.placeholder(),
    );
    differences.exact(&format!("{path}.name"), expected.name(), actual.name());
}

fn compare_atlas_region(
    differences: &mut Differences,
    path: &str,
    expected: &SemanticAtlasRegion,
    actual: &SemanticAtlasRegion,
) {
    differences.exact(&format!("{path}.page"), expected.page(), actual.page());
    differences.exact(
        &format!("{path}.region"),
        expected.region(),
        actual.region(),
    );
    differences.exact(
        &format!("{path}.sequence_index"),
        &expected.sequence_index(),
        &actual.sequence_index(),
    );
}

fn compare_pair(
    differences: &mut Differences,
    path: &str,
    expected: [f32; 2],
    actual: [f32; 2],
    tolerance: f64,
) {
    for index in 0..2 {
        differences.numeric(
            &format!("{path}[{index}]"),
            expected[index],
            actual[index],
            tolerance,
        );
    }
}

fn compare_quad(
    differences: &mut Differences,
    path: &str,
    expected: [f32; 4],
    actual: [f32; 4],
    tolerance: f64,
) {
    for index in 0..4 {
        differences.numeric(
            &format!("{path}[{index}]"),
            expected[index],
            actual[index],
            tolerance,
        );
    }
}

fn compare_vec2_slice(
    differences: &mut Differences,
    path: &str,
    expected: &[[f32; 2]],
    actual: &[[f32; 2]],
    tolerance: f64,
) {
    differences.length(path, expected.len(), actual.len());
    for (vertex, (expected_pair, actual_pair)) in expected.iter().zip(actual).enumerate() {
        compare_pair(
            differences,
            &format!("{path}[{vertex}]"),
            *expected_pair,
            *actual_pair,
            tolerance,
        );
    }
}

fn compare_optional_vec2_slice(
    differences: &mut Differences,
    path: &str,
    expected: Option<&[[f32; 2]]>,
    actual: Option<&[[f32; 2]]>,
    tolerance: f64,
) {
    match (expected, actual) {
        (Some(expected), Some(actual)) => {
            compare_vec2_slice(differences, path, expected, actual, tolerance);
        }
        (None, None) => {}
        _ => differences.exact(path, &expected, &actual),
    }
}

fn compare_u32_slice(differences: &mut Differences, path: &str, expected: &[u32], actual: &[u32]) {
    differences.length(path, expected.len(), actual.len());
    for (index, (expected_index, actual_index)) in expected.iter().zip(actual).enumerate() {
        differences.exact(&format!("{path}[{index}]"), expected_index, actual_index);
    }
}

fn bounded_debug<T>(value: &T, maximum_bytes: usize) -> Box<str>
where
    T: Debug + ?Sized,
{
    let mut output = BoundedText::new(maximum_bytes);
    let _ = write!(&mut output, "{value:?}");
    output.finish()
}

fn bounded_plain(value: &str, maximum_bytes: usize) -> Box<str> {
    let mut output = BoundedText::new(maximum_bytes);
    let _ = output.write_str(value);
    output.finish()
}

struct BoundedText {
    value: String,
    maximum_bytes: usize,
    truncated: bool,
}

impl BoundedText {
    const ELLIPSIS: &'static str = "…";

    fn new(maximum_bytes: usize) -> Self {
        Self {
            value: String::with_capacity(maximum_bytes),
            maximum_bytes,
            truncated: false,
        }
    }

    fn finish(mut self) -> Box<str> {
        if self.truncated && self.maximum_bytes >= Self::ELLIPSIS.len() {
            while self.value.len() + Self::ELLIPSIS.len() > self.maximum_bytes {
                self.value.pop();
            }
            self.value.push_str(Self::ELLIPSIS);
        }
        self.value.into_boxed_str()
    }
}

impl Write for BoundedText {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.truncated || value.is_empty() {
            return Ok(());
        }
        let remaining = self.maximum_bytes.saturating_sub(self.value.len());
        if value.len() <= remaining {
            self.value.push_str(value);
            return Ok(());
        }

        let mut boundary = remaining;
        while boundary > 0 && !value.is_char_boundary(boundary) {
            boundary -= 1;
        }
        self.value.push_str(&value[..boundary]);
        self.truncated = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Default)]
    struct NumericFixture {
        local_translation: [f32; 2],
        local_rotation: f32,
        local_scale: [f32; 2],
        local_shear: [f32; 2],
        world_translation: [f32; 2],
        world_x_axis: [f32; 2],
        world_y_axis: [f32; 2],
        slot_color: [f32; 4],
        draw_positions: [[f32; 2]; 4],
        draw_uvs: [[f32; 2]; 4],
        draw_color: [f32; 4],
    }

    #[test]
    fn identical_frames_have_no_differences() {
        let frame = fixture(NumericFixture::default());
        let comparison = compare_semantic_frames(&frame, &frame);

        assert_eq!(comparison.format_version(), 1);
        assert!(comparison.differences().is_empty());
        assert_eq!(comparison.omitted_difference_count(), 0);
    }

    #[test]
    fn every_numeric_leaf_uses_its_fixed_boundary() {
        struct Case {
            path: &'static str,
            tolerance: f64,
            set: fn(&mut NumericFixture, f32),
        }

        let cases = [
            Case {
                path: "bones[0].local.translation[0]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.local_translation[0] = value,
            },
            Case {
                path: "bones[0].local.translation[1]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.local_translation[1] = value,
            },
            Case {
                path: "bones[0].local.rotation_radians",
                tolerance: ANGLE_RADIANS_ABS,
                set: |frame, value| frame.local_rotation = value,
            },
            Case {
                path: "bones[0].local.scale[0]",
                tolerance: UNITLESS_ABS,
                set: |frame, value| frame.local_scale[0] = value,
            },
            Case {
                path: "bones[0].local.scale[1]",
                tolerance: UNITLESS_ABS,
                set: |frame, value| frame.local_scale[1] = value,
            },
            Case {
                path: "bones[0].local.shear_radians[0]",
                tolerance: ANGLE_RADIANS_ABS,
                set: |frame, value| frame.local_shear[0] = value,
            },
            Case {
                path: "bones[0].local.shear_radians[1]",
                tolerance: ANGLE_RADIANS_ABS,
                set: |frame, value| frame.local_shear[1] = value,
            },
            Case {
                path: "bones[0].world.translation[0]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.world_translation[0] = value,
            },
            Case {
                path: "bones[0].world.translation[1]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.world_translation[1] = value,
            },
            Case {
                path: "bones[0].world.x_axis[0]",
                tolerance: AXIS_ABS,
                set: |frame, value| frame.world_x_axis[0] = value,
            },
            Case {
                path: "bones[0].world.x_axis[1]",
                tolerance: AXIS_ABS,
                set: |frame, value| frame.world_x_axis[1] = value,
            },
            Case {
                path: "bones[0].world.y_axis[0]",
                tolerance: AXIS_ABS,
                set: |frame, value| frame.world_y_axis[0] = value,
            },
            Case {
                path: "bones[0].world.y_axis[1]",
                tolerance: AXIS_ABS,
                set: |frame, value| frame.world_y_axis[1] = value,
            },
            Case {
                path: "slots[0].color_rgba[0]",
                tolerance: COLOR_ABS,
                set: |frame, value| frame.slot_color[0] = value,
            },
            Case {
                path: "slots[0].color_rgba[1]",
                tolerance: COLOR_ABS,
                set: |frame, value| frame.slot_color[1] = value,
            },
            Case {
                path: "slots[0].color_rgba[2]",
                tolerance: COLOR_ABS,
                set: |frame, value| frame.slot_color[2] = value,
            },
            Case {
                path: "slots[0].color_rgba[3]",
                tolerance: COLOR_ABS,
                set: |frame, value| frame.slot_color[3] = value,
            },
            Case {
                path: "draw_items[0].positions[0][0]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.draw_positions[0][0] = value,
            },
            Case {
                path: "draw_items[0].positions[0][1]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.draw_positions[0][1] = value,
            },
            Case {
                path: "draw_items[0].positions[1][0]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.draw_positions[1][0] = value,
            },
            Case {
                path: "draw_items[0].positions[1][1]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.draw_positions[1][1] = value,
            },
            Case {
                path: "draw_items[0].positions[2][0]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.draw_positions[2][0] = value,
            },
            Case {
                path: "draw_items[0].positions[2][1]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.draw_positions[2][1] = value,
            },
            Case {
                path: "draw_items[0].positions[3][0]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.draw_positions[3][0] = value,
            },
            Case {
                path: "draw_items[0].positions[3][1]",
                tolerance: POSITION_ABS,
                set: |frame, value| frame.draw_positions[3][1] = value,
            },
            Case {
                path: "draw_items[0].uvs[0][0]",
                tolerance: UV_ABS,
                set: |frame, value| frame.draw_uvs[0][0] = value,
            },
            Case {
                path: "draw_items[0].uvs[0][1]",
                tolerance: UV_ABS,
                set: |frame, value| frame.draw_uvs[0][1] = value,
            },
            Case {
                path: "draw_items[0].uvs[1][0]",
                tolerance: UV_ABS,
                set: |frame, value| frame.draw_uvs[1][0] = value,
            },
            Case {
                path: "draw_items[0].uvs[1][1]",
                tolerance: UV_ABS,
                set: |frame, value| frame.draw_uvs[1][1] = value,
            },
            Case {
                path: "draw_items[0].uvs[2][0]",
                tolerance: UV_ABS,
                set: |frame, value| frame.draw_uvs[2][0] = value,
            },
            Case {
                path: "draw_items[0].uvs[2][1]",
                tolerance: UV_ABS,
                set: |frame, value| frame.draw_uvs[2][1] = value,
            },
            Case {
                path: "draw_items[0].uvs[3][0]",
                tolerance: UV_ABS,
                set: |frame, value| frame.draw_uvs[3][0] = value,
            },
            Case {
                path: "draw_items[0].uvs[3][1]",
                tolerance: UV_ABS,
                set: |frame, value| frame.draw_uvs[3][1] = value,
            },
            Case {
                path: "draw_items[0].color_rgba[0]",
                tolerance: COLOR_ABS,
                set: |frame, value| frame.draw_color[0] = value,
            },
            Case {
                path: "draw_items[0].color_rgba[1]",
                tolerance: COLOR_ABS,
                set: |frame, value| frame.draw_color[1] = value,
            },
            Case {
                path: "draw_items[0].color_rgba[2]",
                tolerance: COLOR_ABS,
                set: |frame, value| frame.draw_color[2] = value,
            },
            Case {
                path: "draw_items[0].color_rgba[3]",
                tolerance: COLOR_ABS,
                set: |frame, value| frame.draw_color[3] = value,
            },
        ];
        let expected = fixture(NumericFixture::default());

        for case in cases {
            // The schema stores f32 values. The f32 encoding of each declared
            // decimal boundary is the last representable value still within
            // policy; the next bit-pattern is the first value outside it.
            let boundary = case.tolerance as f32;
            assert!(f64::from(boundary) <= case.tolerance);
            let just_over = f32::from_bits(boundary.to_bits() + 1);
            assert!(f64::from(just_over) > case.tolerance);

            let mut at_boundary = NumericFixture::default();
            (case.set)(&mut at_boundary, boundary);
            assert_paths(
                &expected,
                &fixture(at_boundary),
                &[],
                &format!("{} at boundary", case.path),
            );

            let mut outside = NumericFixture::default();
            (case.set)(&mut outside, just_over);
            assert_paths(
                &expected,
                &fixture(outside),
                &[case.path],
                &format!("{} just over boundary", case.path),
            );
        }
    }

    #[test]
    fn every_exact_leaf_and_option_has_a_stable_path() {
        struct Case {
            label: &'static str,
            from: &'static str,
            to: &'static str,
            paths: &'static [&'static str],
        }

        let cases = [
            Case {
                label: "default skin option",
                from: "\"default_skin\":\"default\"",
                to: "\"default_skin\":null",
                paths: &["default_skin"],
            },
            Case {
                label: "skin layer name",
                from: "\"skin_layers\":[\"layer-a\",\"layer-b\"]",
                to: "\"skin_layers\":[\"other\",\"layer-b\"]",
                paths: &["skin_layers[0]"],
            },
            Case {
                label: "bone name",
                from: "\"ordinal\":0,\"name\":\"root\"",
                to: "\"ordinal\":0,\"name\":\"other-root\"",
                paths: &["bones[0].name"],
            },
            Case {
                label: "slot name",
                from: "\"draw_order\":0,\"name\":\"body-slot\"",
                to: "\"draw_order\":0,\"name\":\"other-slot\"",
                paths: &["slots[0].name"],
            },
            Case {
                label: "slot attachment option",
                from: "\"attachment\":{\"skin\":\"slot-skin\",\"slot\":\"slot-owner\",\"placeholder\":\"slot-placeholder\",\"name\":\"slot-attachment\"}",
                to: "\"attachment\":null",
                paths: &["slots[0].attachment"],
            },
            Case {
                label: "slot attachment skin",
                from: "\"skin\":\"slot-skin\"",
                to: "\"skin\":\"other-slot-skin\"",
                paths: &["slots[0].attachment.skin"],
            },
            Case {
                label: "slot attachment slot",
                from: "\"slot\":\"slot-owner\"",
                to: "\"slot\":\"other-slot-owner\"",
                paths: &["slots[0].attachment.slot"],
            },
            Case {
                label: "slot attachment placeholder",
                from: "\"placeholder\":\"slot-placeholder\"",
                to: "\"placeholder\":\"other-slot-placeholder\"",
                paths: &["slots[0].attachment.placeholder"],
            },
            Case {
                label: "slot attachment name",
                from: "\"name\":\"slot-attachment\"",
                to: "\"name\":\"other-slot-attachment\"",
                paths: &["slots[0].attachment.name"],
            },
            Case {
                label: "draw kind enum",
                from: "\"kind\":\"mesh\"",
                to: "\"kind\":\"region\"",
                paths: &["draw_items[0].kind"],
            },
            Case {
                label: "draw slot",
                from: "\"kind\":\"mesh\",\"slot\":\"draw-slot\"",
                to: "\"kind\":\"mesh\",\"slot\":\"other-draw-slot\"",
                paths: &["draw_items[0].slot"],
            },
            Case {
                label: "draw attachment skin",
                from: "\"skin\":\"draw-skin\"",
                to: "\"skin\":\"other-draw-skin\"",
                paths: &["draw_items[0].attachment.skin"],
            },
            Case {
                label: "draw attachment slot",
                from: "\"slot\":\"draw-owner\"",
                to: "\"slot\":\"other-draw-owner\"",
                paths: &["draw_items[0].attachment.slot"],
            },
            Case {
                label: "draw attachment placeholder",
                from: "\"placeholder\":\"draw-placeholder\"",
                to: "\"placeholder\":\"other-draw-placeholder\"",
                paths: &["draw_items[0].attachment.placeholder"],
            },
            Case {
                label: "draw attachment name",
                from: "\"name\":\"draw-attachment\"",
                to: "\"name\":\"other-draw-attachment\"",
                paths: &["draw_items[0].attachment.name"],
            },
            Case {
                label: "atlas page",
                from: "\"page\":\"page.png\"",
                to: "\"page\":\"other.png\"",
                paths: &["draw_items[0].atlas_region.page"],
            },
            Case {
                label: "atlas region",
                from: "\"region\":\"body-region\"",
                to: "\"region\":\"other-region\"",
                paths: &["draw_items[0].atlas_region.region"],
            },
            Case {
                label: "atlas sequence option",
                from: "\"sequence_index\":null",
                to: "\"sequence_index\":2",
                paths: &["draw_items[0].atlas_region.sequence_index"],
            },
            Case {
                label: "blend enum",
                from: "\"blend_mode\":\"normal\"",
                to: "\"blend_mode\":\"additive\"",
                paths: &["draw_items[0].blend_mode"],
            },
            Case {
                label: "uv option",
                from: "\"uvs\":[[0,0],[0,0],[0,0],[0,0]]",
                to: "\"uvs\":null",
                paths: &["draw_items[0].uvs"],
            },
            Case {
                label: "triangle index",
                from: "\"triangles\":[0,1,2,0,2,3]",
                to: "\"triangles\":[0,1,2,0,1,3]",
                paths: &["draw_items[0].triangles[4]"],
            },
            Case {
                label: "IK name",
                from: "\"name\":\"aim\",\"active\":false",
                to: "\"name\":\"other-aim\",\"active\":false",
                paths: &["ik_constraints[0].name"],
            },
            Case {
                label: "IK active",
                from: "\"name\":\"aim\",\"active\":false",
                to: "\"name\":\"aim\",\"active\":true",
                paths: &["ik_constraints[0].active"],
            },
            Case {
                label: "IK preserved status",
                from: "\"preserved_underdetermined\":false",
                to: "\"preserved_underdetermined\":true",
                paths: &["ik_constraints[0].preserved_underdetermined"],
            },
            Case {
                label: "IK target enum",
                from: "\"target_reach\":\"reachable\"",
                to: "\"target_reach\":\"beyond_reach\"",
                paths: &["ik_constraints[0].target_reach"],
            },
            Case {
                label: "IK child translation status",
                from: "\"child_translation_y_zeroed\":false",
                to: "\"child_translation_y_zeroed\":true",
                paths: &["ik_constraints[0].child_translation_y_zeroed"],
            },
            Case {
                label: "IK issue option",
                from: "\"child_translation_y_zeroed\":false,\"issue\":null",
                to: "\"child_translation_y_zeroed\":false,\"issue\":\"singular_or_underdetermined\"",
                paths: &["ik_constraints[0].issue"],
            },
            Case {
                label: "transform name",
                from: "\"name\":\"follow\",\"active\":false",
                to: "\"name\":\"other-follow\",\"active\":false",
                paths: &["transform_constraints[0].name"],
            },
            Case {
                label: "transform active",
                from: "\"name\":\"follow\",\"active\":false",
                to: "\"name\":\"follow\",\"active\":true",
                paths: &["transform_constraints[0].active"],
            },
            Case {
                label: "transform issue option",
                from: "\"name\":\"follow\",\"active\":false,\"issue\":null",
                to: "\"name\":\"follow\",\"active\":false,\"issue\":\"singular_or_underdetermined\"",
                paths: &["transform_constraints[0].issue"],
            },
            Case {
                label: "diagnostic severity enum",
                from: "\"severity\":\"warning\"",
                to: "\"severity\":\"degraded\"",
                paths: &["active_diagnostics[0].severity"],
            },
            Case {
                label: "diagnostic code enum",
                from: "\"code\":\"unknown_field\"",
                to: "\"code\":\"untested_patch_version\"",
                paths: &["active_diagnostics[0].code"],
            },
            Case {
                label: "diagnostic scope kind",
                from: "\"scope\":{\"kind\":\"bone\",\"value\":\"scope-root\"}",
                to: "\"scope\":{\"kind\":\"slot\",\"value\":\"scope-root\"}",
                paths: &["active_diagnostics[0].scope.kind"],
            },
            Case {
                label: "diagnostic scope value",
                from: "\"value\":\"scope-root\"",
                to: "\"value\":\"other-scope-root\"",
                paths: &["active_diagnostics[0].scope.value"],
            },
            Case {
                label: "diagnostic message",
                from: "\"message\":\"fixture warning\"",
                to: "\"message\":\"other warning\"",
                paths: &["active_diagnostics[0].message"],
            },
        ];
        let expected_json = fixture_json(NumericFixture::default());
        let expected = parse_frame(&expected_json);

        for case in cases {
            let actual_json = mutate_once(&expected_json, case.from, case.to, case.label);
            assert_paths(
                &expected,
                &parse_frame(&actual_json),
                case.paths,
                case.label,
            );
        }
    }

    #[test]
    fn schema_version_and_ordinal_fields_are_parser_invariants() {
        let valid = fixture_json(NumericFixture::default());
        for (label, from, to) in [
            (
                "format version",
                "\"format_version\":1",
                "\"format_version\":2",
            ),
            ("bone ordinal", "\"ordinal\":0", "\"ordinal\":1"),
            ("slot draw order", "\"draw_order\":0", "\"draw_order\":1"),
        ] {
            let invalid = mutate_once(&valid, from, to, label);
            assert!(
                SemanticFrame::from_json(invalid.as_bytes()).is_err(),
                "{label} must be rejected before comparison"
            );
            assert!(matches!(
                compare_semantic_frame_json(invalid.as_bytes(), valid.as_bytes()),
                Err(SemanticCompareError::InvalidExpectedFrame { .. })
            ));
            assert!(matches!(
                compare_semantic_frame_json(valid.as_bytes(), invalid.as_bytes()),
                Err(SemanticCompareError::InvalidActualFrame { .. })
            ));
        }
    }

    #[test]
    fn every_semantic_list_reports_length_changes() {
        let expected_json = fixture_json(NumericFixture::default());
        let expected = parse_frame(&expected_json);
        for (key, next_key, path) in [
            ("skin_layers", Some("bones"), "skin_layers.length"),
            ("bones", Some("slots"), "bones.length"),
            ("slots", Some("draw_items"), "slots.length"),
            ("draw_items", Some("ik_constraints"), "draw_items.length"),
            (
                "ik_constraints",
                Some("transform_constraints"),
                "ik_constraints.length",
            ),
            (
                "transform_constraints",
                Some("active_diagnostics"),
                "transform_constraints.length",
            ),
            ("active_diagnostics", None, "active_diagnostics.length"),
        ] {
            let actual_json = replace_top_level_array(&expected_json, key, next_key, "");
            assert_paths(
                &expected,
                &parse_frame(&actual_json),
                &[path],
                &format!("{key} length"),
            );
        }

        let longer_geometry = mutate_once(
            &mutate_once(
                &expected_json,
                "\"positions\":[[0,0],[0,0],[0,0],[0,0]]",
                "\"positions\":[[0,0],[0,0],[0,0],[0,0],[0,0]]",
                "positions length",
            ),
            "\"uvs\":[[0,0],[0,0],[0,0],[0,0]]",
            "\"uvs\":[[0,0],[0,0],[0,0],[0,0],[0,0]]",
            "UV length",
        );
        assert_paths(
            &expected,
            &parse_frame(&longer_geometry),
            &["draw_items[0].positions.length", "draw_items[0].uvs.length"],
            "geometry lengths",
        );

        let longer_triangles = mutate_once(
            &expected_json,
            "\"triangles\":[0,1,2,0,2,3]",
            "\"triangles\":[0,1,2,0,2,3,0,1,2]",
            "triangle length",
        );
        assert_paths(
            &expected,
            &parse_frame(&longer_triangles),
            &["draw_items[0].triangles.length"],
            "triangle length",
        );
    }

    #[test]
    fn every_ordered_semantic_list_compares_by_index() {
        let base = fixture_json(NumericFixture::default());

        let swapped_layers = mutate_once(
            &base,
            "\"skin_layers\":[\"layer-a\",\"layer-b\"]",
            "\"skin_layers\":[\"layer-b\",\"layer-a\"]",
            "skin layer order",
        );
        assert_json_paths(
            &base,
            &swapped_layers,
            &["skin_layers[0]", "skin_layers[1]"],
            "skin layer order",
        );

        let first_bone = top_level_array(&base, "bones", Some("slots"));
        let second_bone = mutate_once(
            &mutate_once(
                first_bone,
                "\"ordinal\":0",
                "\"ordinal\":1",
                "second bone ordinal",
            ),
            "\"name\":\"root\"",
            "\"name\":\"child\"",
            "second bone name",
        );
        let expected_bones = replace_top_level_array(
            &base,
            "bones",
            Some("slots"),
            &format!("{first_bone},{second_bone}"),
        );
        let actual_bones = replace_top_level_array(
            &base,
            "bones",
            Some("slots"),
            &format!(
                "{},{}",
                mutate_once(
                    first_bone,
                    "\"name\":\"root\"",
                    "\"name\":\"child\"",
                    "first swapped bone",
                ),
                mutate_once(
                    &second_bone,
                    "\"name\":\"child\"",
                    "\"name\":\"root\"",
                    "second swapped bone",
                ),
            ),
        );
        assert_json_paths(
            &expected_bones,
            &actual_bones,
            &["bones[0].name", "bones[1].name"],
            "bone order",
        );

        let first_slot = top_level_array(&base, "slots", Some("draw_items"));
        let second_slot = mutate_once(
            &mutate_once(
                first_slot,
                "\"draw_order\":0",
                "\"draw_order\":1",
                "second slot draw order",
            ),
            "\"name\":\"body-slot\"",
            "\"name\":\"other-slot\"",
            "second slot name",
        );
        let expected_slots = replace_top_level_array(
            &base,
            "slots",
            Some("draw_items"),
            &format!("{first_slot},{second_slot}"),
        );
        let actual_slots = replace_top_level_array(
            &base,
            "slots",
            Some("draw_items"),
            &format!(
                "{},{}",
                mutate_once(
                    first_slot,
                    "\"name\":\"body-slot\"",
                    "\"name\":\"other-slot\"",
                    "first swapped slot",
                ),
                mutate_once(
                    &second_slot,
                    "\"name\":\"other-slot\"",
                    "\"name\":\"body-slot\"",
                    "second swapped slot",
                ),
            ),
        );
        assert_json_paths(
            &expected_slots,
            &actual_slots,
            &["slots[0].name", "slots[1].name"],
            "slot order",
        );

        for (key, next_key, identity_from, identity_second, expected_paths) in [
            (
                "draw_items",
                Some("ik_constraints"),
                "\"slot\":\"draw-slot\"",
                "\"slot\":\"other-draw-slot\"",
                ["draw_items[0].slot", "draw_items[1].slot"],
            ),
            (
                "ik_constraints",
                Some("transform_constraints"),
                "\"name\":\"aim\"",
                "\"name\":\"other-aim\"",
                ["ik_constraints[0].name", "ik_constraints[1].name"],
            ),
            (
                "transform_constraints",
                Some("active_diagnostics"),
                "\"name\":\"follow\"",
                "\"name\":\"other-follow\"",
                [
                    "transform_constraints[0].name",
                    "transform_constraints[1].name",
                ],
            ),
            (
                "active_diagnostics",
                None,
                "\"message\":\"fixture warning\"",
                "\"message\":\"other warning\"",
                [
                    "active_diagnostics[0].message",
                    "active_diagnostics[1].message",
                ],
            ),
        ] {
            let first = top_level_array(&base, key, next_key);
            let second = mutate_once(first, identity_from, identity_second, key);
            let expected_json =
                replace_top_level_array(&base, key, next_key, &format!("{first},{second}"));
            let first_swapped = mutate_once(first, identity_from, identity_second, key);
            let second_swapped = mutate_once(&second, identity_second, identity_from, key);
            let actual_json = replace_top_level_array(
                &base,
                key,
                next_key,
                &format!("{first_swapped},{second_swapped}"),
            );
            assert_json_paths(
                &expected_json,
                &actual_json,
                &expected_paths,
                &format!("{key} order"),
            );
        }
    }

    #[test]
    fn geometry_vectors_compare_every_position_in_order() {
        let expected_values = NumericFixture {
            draw_positions: [[0.1, 0.2], [0.3, 0.4], [0.5, 0.6], [0.7, 0.8]],
            draw_uvs: [[0.11, 0.12], [0.13, 0.14], [0.15, 0.16], [0.17, 0.18]],
            ..NumericFixture::default()
        };
        let mut actual_values = expected_values;
        actual_values.draw_positions.swap(0, 3);
        actual_values.draw_uvs.swap(0, 3);
        assert_paths(
            &fixture(expected_values),
            &fixture(actual_values),
            &[
                "draw_items[0].positions[0][0]",
                "draw_items[0].positions[0][1]",
                "draw_items[0].positions[3][0]",
                "draw_items[0].positions[3][1]",
                "draw_items[0].uvs[0][0]",
                "draw_items[0].uvs[0][1]",
                "draw_items[0].uvs[3][0]",
                "draw_items[0].uvs[3][1]",
            ],
            "geometry order",
        );
    }

    #[test]
    fn every_diagnostic_scope_variant_and_value_is_compared() {
        let base = fixture_json(NumericFixture::default());
        let asset_scope = r#"{"kind":"asset"}"#;
        for (kind, scope) in [
            ("bone", r#"{"kind":"bone","value":"scope-root"}"#),
            ("slot", r#"{"kind":"slot","value":"scope-root"}"#),
            ("skin", r#"{"kind":"skin","value":"scope-root"}"#),
            ("animation", r#"{"kind":"animation","value":"scope-root"}"#),
            ("event", r#"{"kind":"event","value":"scope-root"}"#),
            (
                "attachment",
                r#"{"kind":"attachment","value":{"skin":"scope-skin","slot":"scope-slot","placeholder":"scope-placeholder","name":"scope-attachment"}}"#,
            ),
            (
                "ik_constraint",
                r#"{"kind":"ik_constraint","value":"scope-root"}"#,
            ),
            (
                "constraint",
                r#"{"kind":"constraint","value":"scope-root"}"#,
            ),
            ("atlas_page", r#"{"kind":"atlas_page","value":"page.png"}"#),
            (
                "atlas_region",
                r#"{"kind":"atlas_region","value":{"page":"page.png","region":"scope-region","sequence_index":null}}"#,
            ),
        ] {
            let expected_json = with_diagnostic_scope(&base, scope);
            let actual_json = with_diagnostic_scope(&base, asset_scope);
            let comparison =
                compare_semantic_frames(&parse_frame(&expected_json), &parse_frame(&actual_json));
            assert_eq!(
                comparison
                    .differences()
                    .iter()
                    .map(SemanticDifference::path)
                    .collect::<Vec<_>>(),
                ["active_diagnostics[0].scope.kind"],
                "{kind} scope kind"
            );
            assert_eq!(
                comparison.differences()[0].expected(),
                format!("{kind:?}"),
                "{kind} scope label"
            );
            assert_eq!(comparison.differences()[0].actual(), "\"asset\"");
        }

        for kind in [
            "bone",
            "slot",
            "skin",
            "animation",
            "event",
            "ik_constraint",
            "constraint",
            "atlas_page",
        ] {
            let expected_json =
                with_diagnostic_scope(&base, &format!(r#"{{"kind":"{kind}","value":"expected"}}"#));
            let actual_json =
                with_diagnostic_scope(&base, &format!(r#"{{"kind":"{kind}","value":"actual"}}"#));
            assert_json_paths(
                &expected_json,
                &actual_json,
                &["active_diagnostics[0].scope.value"],
                &format!("{kind} scope value"),
            );
        }

        let attachment_scope = r#"{"kind":"attachment","value":{"skin":"scope-skin","slot":"scope-slot","placeholder":"scope-placeholder","name":"scope-attachment"}}"#;
        for (from, to, path) in [
            (
                "\"skin\":\"scope-skin\"",
                "\"skin\":\"other-skin\"",
                "active_diagnostics[0].scope.value.skin",
            ),
            (
                "\"slot\":\"scope-slot\"",
                "\"slot\":\"other-slot\"",
                "active_diagnostics[0].scope.value.slot",
            ),
            (
                "\"placeholder\":\"scope-placeholder\"",
                "\"placeholder\":\"other-placeholder\"",
                "active_diagnostics[0].scope.value.placeholder",
            ),
            (
                "\"name\":\"scope-attachment\"",
                "\"name\":\"other-attachment\"",
                "active_diagnostics[0].scope.value.name",
            ),
        ] {
            let expected_json = with_diagnostic_scope(&base, attachment_scope);
            let actual_scope = mutate_once(attachment_scope, from, to, path);
            let actual_json = with_diagnostic_scope(&base, &actual_scope);
            assert_json_paths(&expected_json, &actual_json, &[path], path);
        }

        let atlas_scope = r#"{"kind":"atlas_region","value":{"page":"page.png","region":"scope-region","sequence_index":null}}"#;
        for (from, to, path) in [
            (
                "\"page\":\"page.png\"",
                "\"page\":\"other.png\"",
                "active_diagnostics[0].scope.value.page",
            ),
            (
                "\"region\":\"scope-region\"",
                "\"region\":\"other-region\"",
                "active_diagnostics[0].scope.value.region",
            ),
            (
                "\"sequence_index\":null",
                "\"sequence_index\":4",
                "active_diagnostics[0].scope.value.sequence_index",
            ),
        ] {
            let expected_json = with_diagnostic_scope(&base, atlas_scope);
            let actual_scope = mutate_once(atlas_scope, from, to, path);
            let actual_json = with_diagnostic_scope(&base, &actual_scope);
            assert_json_paths(&expected_json, &actual_json, &[path], path);
        }
    }

    #[test]
    fn reports_are_count_and_value_bounded() {
        let expected_layers = (0..300)
            .map(|index| format!("\"expected-{index}-{}\"", "é".repeat(300)))
            .collect::<Vec<_>>()
            .join(",");
        let actual_layers = (0..300)
            .map(|index| format!("\"actual-{index}-{}\"", "é".repeat(300)))
            .collect::<Vec<_>>()
            .join(",");
        let expected = minimal_frame_with_layers(&expected_layers);
        let actual = minimal_frame_with_layers(&actual_layers);
        let comparison = compare_semantic_frames(&expected, &actual);

        assert_eq!(comparison.differences().len(), MAX_SEMANTIC_DIFFERENCES);
        assert_eq!(comparison.omitted_difference_count(), 44);
        for difference in comparison.differences() {
            assert!(difference.path().len() <= MAX_SEMANTIC_DIFFERENCE_PATH_BYTES);
            assert!(difference.expected().len() <= MAX_SEMANTIC_DIFFERENCE_VALUE_BYTES);
            assert!(difference.actual().len() <= MAX_SEMANTIC_DIFFERENCE_VALUE_BYTES);
            assert!(
                difference
                    .expected()
                    .is_char_boundary(difference.expected().len())
            );
            assert!(
                difference
                    .actual()
                    .is_char_boundary(difference.actual().len())
            );
        }
    }

    #[test]
    fn json_entry_point_rejects_non_finite_channels_through_core_parser() {
        let expected_json = fixture_json(NumericFixture::default()).replacen(
            "\"rotation_radians\":0",
            "\"rotation_radians\":1e40",
            1,
        );
        let actual_json = fixture_json(NumericFixture::default());

        assert!(matches!(
            compare_semantic_frame_json(expected_json.as_bytes(), actual_json.as_bytes()),
            Err(SemanticCompareError::InvalidExpectedFrame { .. })
        ));
    }

    fn fixture(values: NumericFixture) -> SemanticFrame {
        parse_frame(&fixture_json(values))
    }

    fn fixture_json(values: NumericFixture) -> String {
        let local_translation = pair_json(values.local_translation);
        let local_rotation = values.local_rotation.to_string();
        let local_scale = pair_json(values.local_scale);
        let local_shear = pair_json(values.local_shear);
        let world_translation = pair_json(values.world_translation);
        let world_x_axis = pair_json(values.world_x_axis);
        let world_y_axis = pair_json(values.world_y_axis);
        let slot_color = quad_json(values.slot_color);
        let draw_positions = pairs_json(values.draw_positions);
        let draw_uvs = pairs_json(values.draw_uvs);
        let draw_color = quad_json(values.draw_color);
        let bone = [
            r#"{"ordinal":0,"name":"root","local":{"translation":"#,
            &local_translation,
            r#","rotation_radians":"#,
            &local_rotation,
            r#","scale":"#,
            &local_scale,
            r#","shear_radians":"#,
            &local_shear,
            r#"},"world":{"translation":"#,
            &world_translation,
            r#","x_axis":"#,
            &world_x_axis,
            r#","y_axis":"#,
            &world_y_axis,
            "}}",
        ]
        .concat();
        let slot = [
            r#"{"draw_order":0,"name":"body-slot","attachment":{"skin":"slot-skin","slot":"slot-owner","placeholder":"slot-placeholder","name":"slot-attachment"},"color_rgba":"#,
            &slot_color,
            "}",
        ]
        .concat();
        let draw = [
            r#"{"kind":"mesh","slot":"draw-slot","attachment":{"skin":"draw-skin","slot":"draw-owner","placeholder":"draw-placeholder","name":"draw-attachment"},"atlas_region":{"page":"page.png","region":"body-region","sequence_index":null},"blend_mode":"normal","positions":"#,
            &draw_positions,
            r#","uvs":"#,
            &draw_uvs,
            r#","triangles":[0,1,2,0,2,3],"color_rgba":"#,
            &draw_color,
            "}",
        ]
        .concat();
        [
            r#"{"format_version":1,"default_skin":"default","skin_layers":["layer-a","layer-b"],"bones":["#,
            &bone,
            r#"],"slots":["#,
            &slot,
            r#"],"draw_items":["#,
            &draw,
            r#"],"ik_constraints":[{"name":"aim","active":false,"preserved_underdetermined":false,"target_reach":"reachable","child_translation_y_zeroed":false,"issue":null}],"transform_constraints":[{"name":"follow","active":false,"issue":null}],"active_diagnostics":[{"severity":"warning","code":"unknown_field","scope":{"kind":"bone","value":"scope-root"},"message":"fixture warning"}]}"#,
        ]
        .concat()
    }

    fn pair_json(values: [f32; 2]) -> String {
        format!("[{},{}]", values[0], values[1])
    }

    fn pairs_json<const N: usize>(values: [[f32; 2]; N]) -> String {
        format!(
            "[{}]",
            values
                .into_iter()
                .map(pair_json)
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    fn quad_json(values: [f32; 4]) -> String {
        format!("[{},{},{},{}]", values[0], values[1], values[2], values[3])
    }

    fn parse_frame(json: &str) -> SemanticFrame {
        SemanticFrame::from_json(json.as_bytes())
            .expect("the test fixture is a valid semantic frame")
    }

    fn mutate_once(json: &str, from: &str, to: &str, label: &str) -> String {
        assert_eq!(
            json.matches(from).count(),
            1,
            "{label}: mutation source must occur exactly once"
        );
        json.replacen(from, to, 1)
    }

    fn top_level_array<'a>(json: &'a str, key: &str, next_key: Option<&str>) -> &'a str {
        let start_marker = format!("\"{key}\":[");
        let start = json
            .find(&start_marker)
            .unwrap_or_else(|| panic!("{key}: top-level array start must exist"))
            + start_marker.len();
        let end_marker = next_key.map_or_else(|| "]}".to_owned(), |next| format!("],\"{next}\":"));
        let end = json[start..]
            .find(&end_marker)
            .unwrap_or_else(|| panic!("{key}: top-level array end must exist"))
            + start;
        &json[start..end]
    }

    fn replace_top_level_array(
        json: &str,
        key: &str,
        next_key: Option<&str>,
        replacement: &str,
    ) -> String {
        let current = top_level_array(json, key, next_key);
        let start = current.as_ptr() as usize - json.as_ptr() as usize;
        let mut output = String::with_capacity(json.len() - current.len() + replacement.len());
        output.push_str(&json[..start]);
        output.push_str(replacement);
        output.push_str(&json[start + current.len()..]);
        output
    }

    fn assert_json_paths(
        expected_json: &str,
        actual_json: &str,
        expected_paths: &[&str],
        context: &str,
    ) {
        assert_paths(
            &parse_frame(expected_json),
            &parse_frame(actual_json),
            expected_paths,
            context,
        );
    }

    fn with_diagnostic_scope(json: &str, scope: &str) -> String {
        mutate_once(
            json,
            r#""scope":{"kind":"bone","value":"scope-root"}"#,
            &format!(r#""scope":{scope}"#),
            "diagnostic scope",
        )
    }

    fn assert_paths(
        expected: &SemanticFrame,
        actual: &SemanticFrame,
        expected_paths: &[&str],
        context: &str,
    ) {
        let comparison = compare_semantic_frames(expected, actual);
        let actual_paths = comparison
            .differences()
            .iter()
            .map(SemanticDifference::path)
            .collect::<Vec<_>>();
        assert_eq!(actual_paths, expected_paths, "{context}");
        assert_eq!(comparison.omitted_difference_count(), 0, "{context}");
    }

    fn minimal_frame_with_layers(layers: &str) -> SemanticFrame {
        let json = format!(
            r#"{{"format_version":1,"default_skin":null,"skin_layers":[{layers}],"bones":[],"slots":[],"draw_items":[],"ik_constraints":[],"transform_constraints":[],"active_diagnostics":[]}}"#
        );
        SemanticFrame::from_json(json.as_bytes()).expect("the generated layers are valid")
    }
}

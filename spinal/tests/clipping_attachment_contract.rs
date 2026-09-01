//! Public contract tests for parsing and representing `"clipping"`
//! attachments.
//!
//! Fixture provenance: every fixture below is a minimal, purpose-built
//! (non-oracle) JSON literal, hand-authored directly against the documented
//! Spine 4.3.23 wire format rather than round-tripped through a licensed
//! editor install. It contains no licensee identity, no third-party
//! artwork, and no redistributed editor assets; every number in it is
//! project-owned. See `fixtures/COVERAGE.toml` (id
//! `clipping-attachment-project-owned`) for the ledger entry.
//!
//! Spinal's draw path does not apply clipping -- rendered output is not
//! masked, and every clipping attachment that Spinal *can* parse still
//! raises a degraded [`spinal::DiagnosticCode::UnsupportedClipRendering`]
//! diagnostic to keep that gap non-silent. These tests cover the loader's
//! parse-and-represent behavior only.

use std::sync::Arc;

use spinal::{
    AttachmentKind, DiagnosticCode, DiagnosticScope, DiagnosticSeverity, LoadErrorKind,
    SkeletonAsset, glam::Vec2, load_json,
};

const CLIP_ATLAS: &[u8] = b"page.png\n\tsize: 1, 1\n";

/// A plain (unweighted) clipping attachment with the two editor-derived
/// fields (`color`, `convex`) that a real 4.3.23 export always adds on top
/// of the structural `end`/`vertexCount`/`vertices` fields.
const CLIP_JSON: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [ { "name": "root" } ],
  "slots": [
    { "name": "content", "bone": "root" },
    { "name": "clip", "bone": "root", "attachment": "clip" }
  ],
  "skins": [
    {
      "name": "default",
      "attachments": {
        "clip": {
          "clip": {
            "type": "clipping",
            "end": "content",
            "vertexCount": 4,
            "vertices": [ -12.5, -6.25, 12.5, -6.25, 9.0, 8.5, -9.0, 8.5 ],
            "color": "ce3a3aff",
            "convex": true
          }
        }
      }
    }
  ]
}"#;

/// Identical to [`CLIP_JSON`] except it omits the editor-derived `color` and
/// `convex` fields entirely, to prove their presence does not change
/// parsing.
const CLIP_NO_EDITOR_FIELDS_JSON: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [ { "name": "root" } ],
  "slots": [
    { "name": "content", "bone": "root" },
    { "name": "clip", "bone": "root", "attachment": "clip" }
  ],
  "skins": [
    {
      "name": "default",
      "attachments": {
        "clip": {
          "clip": {
            "type": "clipping",
            "end": "content",
            "vertexCount": 4,
            "vertices": [ -12.5, -6.25, 12.5, -6.25, 9.0, 8.5, -9.0, 8.5 ]
          }
        }
      }
    }
  ]
}"#;

/// A weighted clipping attachment: `vertices` uses the same bone-count-
/// prefixed encoding as a weighted mesh (one influence per vertex here),
/// which is longer than the plain `2 * vertexCount` flat form.
const WEIGHTED_CLIP_JSON: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [ { "name": "root" } ],
  "slots": [
    { "name": "content", "bone": "root" },
    { "name": "clip", "bone": "root", "attachment": "clip" }
  ],
  "skins": [
    {
      "name": "default",
      "attachments": {
        "clip": {
          "clip": {
            "type": "clipping",
            "end": "content",
            "vertexCount": 3,
            "vertices": [
              1, 0, -2.0, -2.0, 1.0,
              1, 0,  2.0, -2.0, 1.0,
              1, 0,  0.0,  2.0, 1.0
            ]
          }
        }
      }
    }
  ]
}"#;

/// Longer than the unweighted `2 * vertexCount` encoding, but too short to
/// be a well-formed weighted stream. `vertexCount` is 3, so the unweighted
/// encoding needs exactly 6 values and a well-formed weighted stream needs
/// at least `5 * 3 = 15` (one minimal `[boneCount, bone, x, y, weight]`
/// group per vertex). This fixture's 8 values -- `2 * vertexCount + 2` --
/// are invalid under both: the first vertex's single declared influence
/// parses cleanly, but the second vertex's declared bone count then
/// overruns the remaining stream. This must fail loudly rather than being
/// mislabeled as a (incorrectly assumed well-formed) weighted clip.
const MALFORMED_WEIGHTED_LENGTH_CLIP_JSON: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [ { "name": "root" } ],
  "slots": [
    { "name": "content", "bone": "root" },
    { "name": "clip", "bone": "root", "attachment": "clip" }
  ],
  "skins": [
    {
      "name": "default",
      "attachments": {
        "clip": {
          "clip": {
            "type": "clipping",
            "end": "content",
            "vertexCount": 3,
            "vertices": [ 1, 0, -2.0, -2.0, 1.0, 1, 0, 2.0 ]
          }
        }
      }
    }
  ]
}"#;

/// Three decoy slots are declared before `content`, so `content` resolves
/// to a non-zero slot index. Every other fixture in this file happens to
/// declare its `end` target first, so none of them can catch an "end
/// resolution always returns slot 0" bug -- this one specifically can.
const MULTI_SLOT_CLIP_JSON: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [ { "name": "root" } ],
  "slots": [
    { "name": "decoy-a", "bone": "root" },
    { "name": "decoy-b", "bone": "root" },
    { "name": "decoy-c", "bone": "root" },
    { "name": "content", "bone": "root" },
    { "name": "clip", "bone": "root", "attachment": "clip" }
  ],
  "skins": [
    {
      "name": "default",
      "attachments": {
        "clip": {
          "clip": {
            "type": "clipping",
            "end": "content",
            "vertexCount": 4,
            "vertices": [ -12.5, -6.25, 12.5, -6.25, 9.0, 8.5, -9.0, 8.5 ]
          }
        }
      }
    }
  ]
}"#;

/// `end` names the `clip` slot itself, which is legal in Spine. Structurally
/// sound here because every slot (including the clip's own) resolves
/// before any attachment is parsed.
const SELF_ENDING_CLIP_JSON: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [ { "name": "root" } ],
  "slots": [
    { "name": "clip", "bone": "root", "attachment": "clip" }
  ],
  "skins": [
    {
      "name": "default",
      "attachments": {
        "clip": {
          "clip": {
            "type": "clipping",
            "end": "clip",
            "vertexCount": 4,
            "vertices": [ -12.5, -6.25, 12.5, -6.25, 9.0, 8.5, -9.0, 8.5 ]
          }
        }
      }
    }
  ]
}"#;

/// A template with an `ATTACHMENT` placeholder for the malformed/missing
/// field error-path table below. `content` and `clip` slots exist so an
/// otherwise-valid clipping attachment always has a resolvable `end`.
const CLIPPING_ERROR_TEMPLATE: &str = r#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [ { "name": "root" } ],
  "slots": [
    { "name": "content", "bone": "root" },
    { "name": "clip", "bone": "root", "attachment": "clip" }
  ],
  "skins": [
    { "name": "default", "attachments": { "clip": { "clip": ATTACHMENT } } }
  ]
}"#;

fn load_clip_asset(json: &[u8]) -> Arc<SkeletonAsset> {
    load_json(json, CLIP_ATLAS)
        .expect("the clipping fixture loads")
        .into_asset()
}

#[test]
fn unweighted_clip_parses_end_slot_and_polygon_exactly() {
    let asset = load_clip_asset(CLIP_JSON);
    let content = asset.slot_id("content").expect("content slot exists");
    let clip = asset
        .attachments()
        .find(|attachment| attachment.name() == "clip")
        .expect("the clip attachment exists");
    assert_eq!(clip.kind(), AttachmentKind::Clipping);

    let clip = clip.as_clipping().expect("typed clipping view");
    assert_eq!(
        clip.end_slot(),
        content,
        "end must resolve to the content slot's id"
    );
    assert_eq!(clip.vertex_count(), 4);
    // Non-trivial (non-zero) coordinates, asserted byte-exact against the
    // authored flat [x, y] list -- proves the standard unweighted vertex
    // encoding is read without transformation (bone-local, as authored).
    assert_eq!(
        clip.vertices(),
        &[
            Vec2::new(-12.5, -6.25),
            Vec2::new(12.5, -6.25),
            Vec2::new(9.0, 8.5),
            Vec2::new(-9.0, 8.5),
        ]
    );
}

#[test]
fn unweighted_clip_degrades_exactly_once_and_load_still_succeeds() {
    let report =
        load_json(CLIP_JSON, CLIP_ATLAS).expect("a clipping attachment must not fail the load");
    // Degraded, not rejected: the load succeeds and is flagged degraded.
    assert!(report.has_degradations());

    let asset = report.asset();
    let clip_id = asset
        .attachments()
        .find(|attachment| attachment.name() == "clip")
        .expect("the clip attachment exists")
        .id();

    let diagnostics = report.diagnostics();
    // Exactly one diagnostic for the whole load: CLIP_JSON also carries the
    // editor-derived `color` and `convex` fields, so this count proves they
    // are tolerated without raising their own (e.g. unknown-field)
    // diagnostic on top of the expected clip-rendering one.
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(
        diagnostics[0].code(),
        DiagnosticCode::UnsupportedClipRendering
    );
    assert_eq!(diagnostics[0].severity(), DiagnosticSeverity::Degraded);
    assert_eq!(diagnostics[0].scope(), DiagnosticScope::Attachment(clip_id));
}

#[test]
fn editor_derived_color_and_convex_are_tolerated_and_parse_identically() {
    let with_fields = load_clip_asset(CLIP_JSON);
    let without_fields = load_clip_asset(CLIP_NO_EDITOR_FIELDS_JSON);

    let with_clip = with_fields
        .attachments()
        .find(|attachment| attachment.name() == "clip")
        .expect("the clip attachment exists")
        .as_clipping()
        .expect("typed clipping view");
    let without_clip = without_fields
        .attachments()
        .find(|attachment| attachment.name() == "clip")
        .expect("the clip attachment exists")
        .as_clipping()
        .expect("typed clipping view");

    assert_eq!(with_clip.vertex_count(), without_clip.vertex_count());
    assert_eq!(with_clip.vertices(), without_clip.vertices());
    // Each end-slot id is compared against a slot lookup on its own asset
    // instance (`SlotId` carries an asset key, so ids from different loads
    // are never equal even when they name the same slot).
    assert_eq!(
        with_clip.end_slot(),
        with_fields.slot_id("content").expect("content exists")
    );
    assert_eq!(
        without_clip.end_slot(),
        without_fields.slot_id("content").expect("content exists")
    );

    // Both loads must produce the identical single degraded diagnostic --
    // presence of the editor-derived fields raises nothing extra.
    let with_report = load_json(CLIP_JSON, CLIP_ATLAS).expect("loads");
    let without_report = load_json(CLIP_NO_EDITOR_FIELDS_JSON, CLIP_ATLAS).expect("loads");
    assert_eq!(with_report.diagnostics().len(), 1);
    assert_eq!(without_report.diagnostics().len(), 1);
    assert_eq!(
        with_report.diagnostics()[0].code(),
        without_report.diagnostics()[0].code()
    );
}

#[test]
fn weighted_clip_stays_unsupported_and_geometry_is_not_exposed() {
    let report = load_json(WEIGHTED_CLIP_JSON, CLIP_ATLAS)
        .expect("a weighted clip must degrade rather than fail the load");
    let diagnostics = report.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(
        diagnostics[0].code(),
        DiagnosticCode::UnsupportedAttachmentType
    );
    assert_eq!(diagnostics[0].severity(), DiagnosticSeverity::Degraded);
    assert!(
        diagnostics[0].message().contains("weighted clipping"),
        "message should name weighted clipping as the reason: {}",
        diagnostics[0].message()
    );

    let asset = report.asset();
    let clip = asset
        .attachments()
        .find(|attachment| attachment.name() == "clip")
        .expect("the clip attachment exists");
    assert_eq!(clip.kind(), AttachmentKind::Unsupported);
    assert_eq!(clip.unsupported_type(), Some("clipping"));
    assert!(
        clip.as_clipping().is_none(),
        "a weighted clip's geometry must not be exposed through the typed view"
    );
}

#[test]
fn clip_vertices_longer_than_unweighted_but_too_short_to_be_weighted_fails_loudly() {
    let error = load_json(MALFORMED_WEIGHTED_LENGTH_CLIP_JSON, CLIP_ATLAS)
        .expect_err("a vertex stream invalid under both encodings must not silently load");
    assert_eq!(error.kind(), LoadErrorKind::SchemaViolation);
    assert!(
        error
            .path()
            .is_some_and(|path| path.ends_with("/vertices/5")),
        "{:?}",
        error.path()
    );
    assert!(
        error.message().contains("truncated"),
        "message should explain the weighted stream is truncated: {}",
        error.message()
    );
}

#[test]
fn clip_end_resolves_to_a_non_zero_slot_index_in_a_multi_slot_skeleton() {
    let asset = load_clip_asset(MULTI_SLOT_CLIP_JSON);
    let content = asset.slot_id("content").expect("content slot exists");
    let decoy_a = asset.slot_id("decoy-a").expect("decoy-a slot exists");
    let clip = asset
        .attachments()
        .find(|attachment| attachment.name() == "clip")
        .expect("the clip attachment exists")
        .as_clipping()
        .expect("typed clipping view");

    // `content` is the fourth declared slot (after three decoys); an
    // "end resolution always returns slot 0" bug would resolve to
    // `decoy_a`, the first declared slot, instead.
    assert_eq!(
        clip.end_slot(),
        content,
        "end must resolve to the content slot specifically, not slot 0"
    );
    assert_ne!(
        clip.end_slot(),
        decoy_a,
        "end must not fall back to the first declared slot"
    );
}

#[test]
fn clip_end_naming_its_own_slot_parses_and_degrades_exactly_once() {
    let report = load_json(SELF_ENDING_CLIP_JSON, CLIP_ATLAS)
        .expect("a clip whose end names its own slot must not fail the load");
    // Degraded, not rejected.
    assert!(report.has_degradations());

    let asset = report.asset();
    let clip_slot = asset.slot_id("clip").expect("clip slot exists");
    let clip = asset
        .attachments()
        .find(|attachment| attachment.name() == "clip")
        .expect("the clip attachment exists");
    let clip_id = clip.id();
    let clip_view = clip.as_clipping().expect("typed clipping view");
    assert_eq!(
        clip_view.end_slot(),
        clip_slot,
        "end may legally name the clip attachment's own slot"
    );

    let diagnostics = report.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(
        diagnostics[0].code(),
        DiagnosticCode::UnsupportedClipRendering
    );
    assert_eq!(diagnostics[0].severity(), DiagnosticSeverity::Degraded);
    assert_eq!(diagnostics[0].scope(), DiagnosticScope::Attachment(clip_id));
}

#[test]
fn malformed_or_missing_clipping_fields_fail_loudly_at_precise_paths() {
    let cases = [
        (
            "missing end",
            r#"{"type":"clipping","vertexCount":4,"vertices":[0,0,1,0,1,1,0,1]}"#,
            LoadErrorKind::SchemaViolation,
            "/end",
        ),
        (
            "missing vertexCount",
            r#"{"type":"clipping","end":"content","vertices":[0,0,1,0,1,1,0,1]}"#,
            LoadErrorKind::SchemaViolation,
            "/vertexCount",
        ),
        (
            "missing vertices",
            r#"{"type":"clipping","end":"content","vertexCount":4}"#,
            LoadErrorKind::SchemaViolation,
            "/vertices",
        ),
        (
            "zero vertexCount",
            r#"{"type":"clipping","end":"content","vertexCount":0,"vertices":[]}"#,
            LoadErrorKind::SchemaViolation,
            "/vertexCount",
        ),
        (
            "truncated vertices",
            r#"{"type":"clipping","end":"content","vertexCount":4,"vertices":[0,0,1,0,1,1]}"#,
            LoadErrorKind::SchemaViolation,
            "/vertices",
        ),
        (
            "unknown end slot",
            r#"{"type":"clipping","end":"does-not-exist","vertexCount":4,"vertices":[0,0,1,0,1,1,0,1]}"#,
            LoadErrorKind::UnresolvedReference,
            "/end",
        ),
    ];

    for (name, attachment_json, expected_kind, path_suffix) in cases {
        let json = CLIPPING_ERROR_TEMPLATE.replace("ATTACHMENT", attachment_json);
        let error = load_json(json.as_bytes(), CLIP_ATLAS).expect_err(name);
        assert_eq!(error.kind(), expected_kind, "{name}");
        assert!(
            error.path().is_some_and(|path| path.ends_with(path_suffix)),
            "{name}: {:?}",
            error.path()
        );
    }
}

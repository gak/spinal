mod animation;
mod build;
pub(crate) mod error;
mod mesh;
mod schema;

pub use error::{LoadDocument, LoadError, LoadErrorKind, SourceLocation};

use std::sync::Arc;

use crate::{
    AnimationId, AtlasPageId, AtlasRegionId, AttachmentId, BoneId, ConstraintId, Diagnostic,
    DiagnosticCode, DiagnosticScope, DiagnosticSeverity, EventId, IkConstraintId, SkeletonAsset,
    SkinId, SlotId, id::AssetKey,
};

/// Loads and links Spine skeleton JSON with its text texture atlas.
///
/// This function performs no filesystem or image I/O. Callers retain control
/// over byte acquisition, page-image resolution, and rendering.
pub fn load_json(skeleton_json: &[u8], atlas_text: &[u8]) -> Result<LoadReport, LoadError> {
    let root = crate::json::parse_json(skeleton_json)?;
    let atlas = crate::atlas::parse_atlas(atlas_text)?;
    let (key, data) = build::build_asset(&root, atlas)?;
    Ok(LoadReport::new(SkeletonAsset::from_data(key, data)))
}

/// A successfully loaded immutable asset and its retained diagnostics.
#[derive(Debug)]
pub struct LoadReport {
    asset: Arc<SkeletonAsset>,
}

impl LoadReport {
    pub(crate) fn new(asset: SkeletonAsset) -> Self {
        Self {
            asset: Arc::new(asset),
        }
    }

    /// Returns the shared immutable asset handle.
    #[must_use]
    pub fn asset(&self) -> &Arc<SkeletonAsset> {
        &self.asset
    }

    /// Returns all non-fatal issues retained by the asset.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.asset.diagnostics()
    }

    /// Returns whether any retained issue changes visible or behavioral output.
    #[must_use]
    pub fn has_degradations(&self) -> bool {
        self.asset.has_degradations()
    }

    /// Consumes the report and returns its shared immutable asset.
    #[must_use]
    pub fn into_asset(self) -> Arc<SkeletonAsset> {
        self.asset
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PendingScope {
    Asset,
    Bone(u32),
    Slot(u32),
    Skin(u32),
    Animation(u32),
    Event(u32),
    Attachment(u32),
    IkConstraint(u32),
    Constraint(u32),
    AtlasPage(u32),
    AtlasRegion(u32),
}

#[derive(Debug)]
pub(crate) struct PendingDiagnostic {
    pub(crate) severity: DiagnosticSeverity,
    pub(crate) code: DiagnosticCode,
    pub(crate) scope: PendingScope,
    pub(crate) message: Box<str>,
}

impl PendingDiagnostic {
    pub(crate) fn degraded(
        code: DiagnosticCode,
        scope: PendingScope,
        message: impl Into<Box<str>>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Degraded,
            code,
            scope,
            message: message.into(),
        }
    }

    pub(crate) fn warning(
        code: DiagnosticCode,
        scope: PendingScope,
        message: impl Into<Box<str>>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            code,
            scope,
            message: message.into(),
        }
    }

    pub(crate) fn materialize(self, key: AssetKey) -> Diagnostic {
        let scope = match self.scope {
            PendingScope::Asset => DiagnosticScope::Asset,
            PendingScope::Bone(index) => DiagnosticScope::Bone(BoneId::new(key, index)),
            PendingScope::Slot(index) => DiagnosticScope::Slot(SlotId::new(key, index)),
            PendingScope::Skin(index) => DiagnosticScope::Skin(SkinId::new(key, index)),
            PendingScope::Animation(index) => {
                DiagnosticScope::Animation(AnimationId::new(key, index))
            }
            PendingScope::Event(index) => DiagnosticScope::Event(EventId::new(key, index)),
            PendingScope::Attachment(index) => {
                DiagnosticScope::Attachment(AttachmentId::new(key, index))
            }
            PendingScope::IkConstraint(index) => {
                DiagnosticScope::IkConstraint(IkConstraintId::new(key, index))
            }
            PendingScope::Constraint(index) => {
                DiagnosticScope::Constraint(ConstraintId::new(key, index))
            }
            PendingScope::AtlasPage(index) => {
                DiagnosticScope::AtlasPage(AtlasPageId::new(key, index))
            }
            PendingScope::AtlasRegion(index) => {
                DiagnosticScope::AtlasRegion(AtlasRegionId::new(key, index))
            }
        };
        Diagnostic {
            severity: self.severity,
            code: self.code,
            scope,
            message: self.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{
        EventDefinitionRef,
        animation::{FrameCurve, TimelineData},
    };

    use super::load_json;

    #[test]
    fn supported_animation_payloads_are_typed_linked_and_retained() {
        let json = br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"upper","parent":"root"},
            {"name":"target","parent":"root"}
          ],
          "slots":[
            {"name":"back","bone":"root","attachment":"body"},
            {"name":"front","bone":"upper","attachment":"alt"}
          ],
          "skins":[{
            "name":"default",
            "attachments":{
              "back":{
                "body":{"width":8,"height":8},
                "alt":{"width":8,"height":8}
              },
              "front":{"alt":{"width":8,"height":8}}
            }
          }],
          "constraints":[{
            "type":"ik",
            "name":"aim",
            "target":"target",
            "bones":["upper"]
          }],
          "events":{
            "step":{"int":7,"float":1.5,"string":"soft","volume":0.75,"balance":-0.25}
          },
          "animations":{
            "action":{
              "bones":{
                "root":{
                  "rotate":[
                    {"value":2,"curve":"stepped"},
                    {"time":1,"value":12}
                  ],
                  "translate":[
                    {"x":1,"y":2,"curve":[0.1,1,0.2,2,0.1,3,0.2,4]},
                    {"time":1,"x":3,"y":4}
                  ],
                  "scale":[{"x":1,"y":1},{"time":1,"x":2,"y":2}],
                  "shear":[{"x":0,"y":0},{"time":1,"x":4,"y":5}]
                }
              },
              "slots":{
                "back":{
                  "attachment":[{"name":"body"},{"time":0.25,"name":"alt"}],
                  "rgba":[
                    {"color":"FFFFFFFF","curve":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]},
                    {"time":0.5,"color":"FF000080"}
                  ]
                }
              },
              "ik":{
                "aim":[
                  {"mix":1,"bendPositive":true,"curve":[0,0,0,0,0,0,0,0]},
                  {"time":0.75,"mix":0.5,"bendPositive":false}
                ]
              },
              "drawOrder":[
                {"offsets":[{"slot":"back","offset":1}]},
                {"time":0.9}
              ],
              "events":[
                {"time":0.4,"name":"step"},
                {"time":0.4,"name":"step","int":9,"string":null}
              ]
            }
          }
        }"#;
        let atlas = b"page.png\nbody\n\tbounds:0,0,8,8\nalt\n\tbounds:8,0,8,8\n";

        let report = load_json(json, atlas).expect("supported animation should load");
        assert!(report.diagnostics().is_empty());
        let asset = report.asset();
        let animation = asset.animation_data(0);
        assert_eq!(animation.name.as_ref(), "action");
        assert_eq!(animation.duration.as_duration(), Duration::from_secs(1));
        assert_eq!(animation.timelines.len(), 9);

        assert!(matches!(
            &animation.timelines[0],
            TimelineData::BoneRotate { frames, .. }
                if frames.len() == 2 && matches!(frames[0].curve, FrameCurve::Stepped)
        ));
        assert!(matches!(
            &animation.timelines[1],
            TimelineData::BoneTranslate { frames, .. }
                if frames.len() == 2
                    && matches!(&frames[0].curve, FrameCurve::Bezier(curves) if curves.len() == 2)
        ));
        assert!(matches!(
            &animation.timelines[4],
            TimelineData::SlotAttachment { frames, .. }
                if frames[1].placeholder_name.as_deref() == Some("alt")
        ));
        assert!(matches!(
            &animation.timelines[5],
            TimelineData::SlotColour { frames, .. }
                if frames[1].colour.alpha() == 0x80
        ));
        assert!(matches!(
            &animation.timelines[6],
            TimelineData::Ik { frames, .. }
                if frames[1].mix.get() == 0.5
                    && frames[1].bend_direction == crate::BendDirection::Negative
        ));
        assert!(matches!(
            &animation.timelines[7],
            TimelineData::DrawOrder { frames }
                if frames[0].offsets[0].offset == 1 && frames[1].offsets.is_empty()
        ));
        assert!(matches!(
            &animation.timelines[8],
            TimelineData::Events { frames }
                if frames.len() == 2
                    && frames[0].payload.integer == 7
                    && frames[1].payload.integer == 9
                    && frames[1].payload.string.is_none()
        ));

        let event = asset.event_definitions().next().expect("event definition");
        assert_eq!(EventDefinitionRef::name(event), "step");
        assert_eq!(event.integer(), 7);
        assert_eq!(event.string(), Some("soft"));
    }
}

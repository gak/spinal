//! Shared presentation of immutable source-inspection findings.

use bevy_spinal::spinal::SemanticDiagnosticSeverity;

use crate::inspection::{InspectionOutcome, SourceInspection};

const MAX_VISIBLE_FINDINGS: usize = 8;

/// Visual urgency for one source's static compatibility findings.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum DiagnosticsTone {
    Compatible,
    Warning,
    Degraded,
}

/// Bounded, host-neutral copy for the native and browser Diagnostics surfaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiagnosticsPresentation {
    tone: DiagnosticsTone,
    compatibility: Box<str>,
    inventory: Box<str>,
    bundle: Box<str>,
    findings: Box<[Box<str>]>,
    omitted_findings: u32,
}

impl DiagnosticsPresentation {
    pub(crate) fn capture(inspection: &SourceInspection) -> Self {
        let diagnostic_count = inspection.diagnostics().len();
        let tone = match (inspection.outcome(), diagnostic_count) {
            (InspectionOutcome::Degraded, _) => DiagnosticsTone::Degraded,
            (InspectionOutcome::Compatible, 0) => DiagnosticsTone::Compatible,
            (InspectionOutcome::Compatible, _) => DiagnosticsTone::Warning,
        };
        let source = inspection.source();
        let counts = *inspection.inventory().counts();
        let compatibility = match tone {
            DiagnosticsTone::Compatible => format!(
                "Compatible — Spine {} matches target {}",
                source.declared_spine_version(),
                source.target_spine_version()
            ),
            DiagnosticsTone::Warning => format!(
                "Compatible with {} — Spine {} (target {})",
                counted(diagnostic_count, "finding", "findings"),
                source.declared_spine_version(),
                source.target_spine_version()
            ),
            DiagnosticsTone::Degraded => format!(
                "Degraded by {} — Spine {} (target {})",
                counted(diagnostic_count, "finding", "findings"),
                source.declared_spine_version(),
                source.target_spine_version()
            ),
        };
        let inventory = format!(
            "{} • {} • {} • {} • {}",
            counted(counts.bones() as usize, "bone", "bones"),
            counted(counts.slots() as usize, "slot", "slots"),
            counted(counts.skins() as usize, "skin", "skins"),
            counted(counts.attachments() as usize, "attachment", "attachments"),
            counted(counts.animations() as usize, "animation", "animations")
        );
        let bundle = format!(
            "{} + {} • {} • {} encoded bytes • {} decoded texture bytes • content {}",
            source.json_path(),
            source.atlas_path(),
            counted(source.file_count() as usize, "file", "files"),
            source.encoded_bytes(),
            source.decoded_texture_bytes(),
            short_digest(source.content_sha256())
        );
        let findings = inspection
            .diagnostics()
            .iter()
            .take(MAX_VISIBLE_FINDINGS)
            .map(|diagnostic| {
                let severity = match diagnostic.severity() {
                    SemanticDiagnosticSeverity::Warning => "Warning",
                    SemanticDiagnosticSeverity::Degraded => "Degraded",
                    _other => "Finding",
                };
                let mut finding = format!(
                    "{severity} at {}: {}",
                    diagnostic.scope(),
                    diagnostic.message()
                );
                if diagnostic.scope_was_truncated() || diagnostic.message_was_truncated() {
                    finding.push_str(" [truncated]");
                }
                finding.into_boxed_str()
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let omitted_findings = u32::try_from(diagnostic_count.saturating_sub(findings.len()))
            .expect("inspection diagnostic limits fit u32");

        Self {
            tone,
            compatibility: compatibility.into_boxed_str(),
            inventory: inventory.into_boxed_str(),
            bundle: bundle.into_boxed_str(),
            findings,
            omitted_findings,
        }
    }

    pub(crate) const fn tone(&self) -> DiagnosticsTone {
        self.tone
    }

    #[cfg(any(feature = "web", test))]
    pub(crate) fn compatibility(&self) -> &str {
        &self.compatibility
    }

    #[cfg(any(feature = "web", test))]
    pub(crate) fn inventory(&self) -> &str {
        &self.inventory
    }

    #[cfg(any(feature = "web", test))]
    pub(crate) fn bundle(&self) -> &str {
        &self.bundle
    }

    #[cfg(any(feature = "web", test))]
    pub(crate) fn findings(&self) -> &[Box<str>] {
        &self.findings
    }

    #[cfg(any(feature = "web", test))]
    pub(crate) const fn omitted_finding_count(&self) -> u32 {
        self.omitted_findings
    }

    #[cfg(any(feature = "native", test))]
    pub(crate) fn compact_text(&self) -> String {
        let mut text = format!("{}\n{}", self.compatibility, self.inventory);
        if let Some(finding) = self.findings.first() {
            text.push('\n');
            text.push_str(finding);
            let remaining = self
                .findings
                .len()
                .saturating_sub(1)
                .saturating_add(self.omitted_findings as usize);
            if remaining > 0 {
                text.push_str(&format!(
                    "\n… {} more; run spinal check for the expanded inspection report",
                    counted(remaining, "finding", "findings")
                ));
            }
        }
        text
    }
}

#[cfg(any(feature = "web", test))]
pub(crate) fn disclosure_summary<'a>(
    presentations: impl IntoIterator<Item = &'a DiagnosticsPresentation>,
) -> String {
    let mut source_count = 0_usize;
    let mut finding_count = 0_usize;
    let mut tone = DiagnosticsTone::Compatible;
    for presentation in presentations {
        source_count += 1;
        finding_count += presentation.findings.len() + presentation.omitted_findings as usize;
        tone = tone.max(presentation.tone);
    }
    match tone {
        DiagnosticsTone::Degraded => "Diagnostics — attention required".to_owned(),
        DiagnosticsTone::Warning => format!(
            "Diagnostics — {}",
            counted(finding_count, "finding", "findings")
        ),
        DiagnosticsTone::Compatible => format!(
            "Diagnostics — {} compatible",
            counted(source_count, "source", "sources")
        ),
    }
}

fn counted(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

fn short_digest(digest: &str) -> &str {
    digest.get(..12).unwrap_or(digest)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use super::*;
    use crate::{bundle::TEST_BLUE_PIXEL_PNG, inspection::SourceInspection};

    const ATLAS: &[u8] = b"rig.png\n\
\tsize: 1, 1\n\
\tformat: RGBA8888\n\
\tfilter: Linear, Linear\n\
\trepeat: none\n\
\tpma: false\n";

    fn inspection(json: &[u8]) -> SourceInspection {
        let json_path = PathBuf::from("rig.json");
        let atlas_path = PathBuf::from("rig.atlas");
        let files = BTreeMap::from([
            (json_path.clone(), json.to_vec()),
            (atlas_path.clone(), ATLAS.to_vec()),
            (PathBuf::from("rig.png"), TEST_BLUE_PIXEL_PNG.to_vec()),
        ]);
        SourceInspection::capture(&crate::bundle::SourceBundle::from_test_files(
            "Diagnostics fixture",
            &json_path,
            &atlas_path,
            files,
        ))
    }

    #[test]
    fn compatible_presentation_is_compact_and_explicit() {
        let inspection = inspection(
            br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}],"slots":[{"name":"body","bone":"root"}],"skins":[{"name":"default"}],"animations":{"idle":{}}}"#,
        );
        let presentation = DiagnosticsPresentation::capture(&inspection);

        assert_eq!(presentation.tone(), DiagnosticsTone::Compatible);
        assert_eq!(
            presentation.compatibility(),
            "Compatible — Spine 4.3.23 matches target 4.3.23"
        );
        assert_eq!(
            presentation.inventory(),
            "1 bone • 1 slot • 1 skin • 0 attachments • 1 animation"
        );
        assert!(
            presentation
                .bundle()
                .starts_with("rig.json + rig.atlas • 3 files")
        );
        assert!(presentation.findings().is_empty());
        assert_eq!(presentation.omitted_finding_count(), 0);
        assert_eq!(
            presentation.compact_text(),
            "Compatible — Spine 4.3.23 matches target 4.3.23\n1 bone • 1 slot • 1 skin • 0 attachments • 1 animation"
        );
    }

    #[test]
    fn degraded_presentation_bounds_rows_and_reports_every_omission() {
        let mut json = String::from(
            r#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"<root>","transform":"onlyTranslation"}]"#,
        );
        for index in 0..12 {
            json.push_str(&format!(r#", "unsupported_{index}":{{}}"#));
        }
        json.push('}');
        let inspection = inspection(json.as_bytes());
        let presentation = DiagnosticsPresentation::capture(&inspection);

        assert_eq!(presentation.tone(), DiagnosticsTone::Degraded);
        assert_eq!(presentation.findings().len(), MAX_VISIBLE_FINDINGS);
        assert!(presentation.omitted_finding_count() > 0);
        let compact = presentation.compact_text();
        assert!(compact.contains("Degraded by"));
        assert!(compact.contains("run spinal check for the expanded inspection report"));
    }

    #[test]
    fn authored_markup_remains_literal_diagnostic_text() {
        let inspection = inspection(
            br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"<script>alert(1)</script>","transform":"onlyTranslation"}]}"#,
        );
        let presentation = DiagnosticsPresentation::capture(&inspection);

        assert!(
            presentation
                .findings()
                .iter()
                .any(|finding| finding.contains("<script>alert(1)</script>"))
        );
    }

    #[test]
    fn aggregate_summary_uses_the_highest_urgency_without_adding_a_mode() {
        let compatible = DiagnosticsPresentation::capture(&inspection(
            br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#,
        ));
        let degraded = DiagnosticsPresentation::capture(&inspection(
            br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root","transform":"onlyTranslation"}]}"#,
        ));

        assert_eq!(
            disclosure_summary([&compatible]),
            "Diagnostics — 1 source compatible"
        );
        assert_eq!(
            disclosure_summary([&compatible, &degraded]),
            "Diagnostics — attention required"
        );
    }
}

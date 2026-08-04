use serde::Serialize;

pub(crate) const PROFILE_NAME: &str = "loafstead-demo";
pub(crate) const PROFILE_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub(crate) schema_version: u32,
    pub(crate) profile: Profile,
    pub(crate) status: String,
    pub(crate) source: Option<Source>,
    pub(crate) spine_version: Option<String>,
    pub(crate) summary: Summary,
    pub(crate) readiness: Readiness,
    pub(crate) inventory: Option<Inventory>,
    pub(crate) findings: Vec<Finding>,
    pub(crate) unverified: Vec<Unverified>,
}

impl Report {
    pub(crate) fn new(json: String, atlas: String) -> Self {
        Self {
            schema_version: 1,
            profile: Profile {
                name: PROFILE_NAME,
                version: PROFILE_VERSION,
            },
            status: "pass".to_owned(),
            source: Some(Source { json, atlas }),
            spine_version: None,
            summary: Summary::default(),
            readiness: Readiness::default(),
            inventory: None,
            findings: Vec::new(),
            unverified: unverified_settings(),
        }
    }

    pub(crate) fn tool_error(status: &str, code: &str, message: String) -> Self {
        Self {
            schema_version: 1,
            profile: Profile {
                name: PROFILE_NAME,
                version: PROFILE_VERSION,
            },
            status: status.to_owned(),
            source: None,
            spine_version: None,
            summary: Summary {
                errors: 1,
                warnings: 0,
            },
            readiness: Readiness::default(),
            inventory: None,
            findings: vec![Finding::new(
                "error",
                code,
                "tool",
                message,
                "Correct the command or source bundle and rerun the check.",
            )],
            unverified: Vec::new(),
        }
    }

    pub(crate) fn error(
        &mut self,
        code: impl Into<String>,
        scope: impl Into<String>,
        message: impl Into<String>,
        fix: impl Into<String>,
    ) {
        self.findings
            .push(Finding::new("error", code, scope, message, fix));
    }

    pub(crate) fn warning(
        &mut self,
        code: impl Into<String>,
        scope: impl Into<String>,
        message: impl Into<String>,
        fix: impl Into<String>,
    ) {
        self.findings
            .push(Finding::new("warning", code, scope, message, fix));
    }

    pub(crate) fn source_error(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.error(
            code,
            "source:bundle",
            message,
            "Correct the source bundle and rerun the check.",
        );
        self.finish();
        self.status = "source-error".to_owned();
    }

    pub(crate) fn finish(&mut self) {
        self.summary.errors = self
            .findings
            .iter()
            .filter(|finding| finding.severity == "error")
            .count();
        self.summary.warnings = self
            .findings
            .iter()
            .filter(|finding| finding.severity == "warning")
            .count();
        self.status = if self.summary.errors == 0 {
            "pass".to_owned()
        } else {
            "fail".to_owned()
        };
    }

    pub(crate) fn exit_code(&self) -> u8 {
        match self.status.as_str() {
            "pass" => 0,
            "fail" => 1,
            "source-error" | "command-error" => 2,
            _other => 3,
        }
    }

    pub(crate) fn render_human(&self) -> String {
        let mut output = String::new();
        let heading = match self.status.as_str() {
            "pass" => "PASS",
            "fail" => "FAIL",
            "source-error" => "SOURCE ERROR",
            "command-error" => "COMMAND ERROR",
            _other => "INTERNAL ERROR",
        };
        output.push_str(&format!(
            "{heading}  {} v{}\n",
            self.profile.name, self.profile.version
        ));
        if let Some(source) = &self.source {
            output.push_str(&format!("      JSON: {}\n", visible(&source.json)));
            output.push_str(&format!("      Atlas: {}\n", visible(&source.atlas)));
        }
        if let Some(version) = &self.spine_version {
            output.push_str(&format!("      Spine: {}\n", visible(version)));
        }
        output.push_str(&format!(
            "      {}/7 required animations | {} hats, {} collars, {} glasses\n",
            self.readiness.required_animations,
            self.readiness.hats,
            self.readiness.collars,
            self.readiness.glasses
        ));
        if let Some(inventory) = &self.inventory {
            output.push_str(&format!(
                "      {} bones, {} slots, {} attachments, {} skins, {} atlas page(s)\n",
                inventory.bones,
                inventory.slots,
                inventory.attachments,
                inventory.skins,
                inventory.atlas_pages
            ));
            output.push_str(&format!(
                "      meshes: {} weighted, {} unweighted, {} linked | IK: {} | transform: {}\n",
                inventory.weighted_meshes,
                inventory.unweighted_meshes,
                inventory.linked_meshes,
                inventory.ik_constraints,
                inventory.transform_constraints
            ));
            if !inventory.animations.is_empty() {
                let animations = inventory
                    .animations
                    .iter()
                    .map(|animation| {
                        format!(
                            "{} ({:.3}s)",
                            visible(&animation.name),
                            animation.duration_seconds
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                output.push_str(&format!("      animations: {animations}\n"));
            }
        }

        if !self.findings.is_empty() {
            output.push('\n');
        }
        for finding in &self.findings {
            output.push_str(&format!(
                "{} [{}] {}\n    {}\n    Fix: {}\n",
                visible(&finding.severity.to_ascii_uppercase()),
                visible(&finding.code),
                visible(&finding.scope),
                visible(&finding.message),
                visible(&finding.fix)
            ));
        }
        output.push_str(&format!(
            "\nSUMMARY  {} error(s), {} warning(s)\n",
            self.summary.errors, self.summary.warnings
        ));
        output.push_str(
            "         Final files cannot prove bleed, padding, strip-whitespace, cleanup,\n",
        );
        output.push_str(
            "         nonessential-data, editor warnings, or artistic colour intent. Keep the\n",
        );
        output.push_str("         shared export and pack presets with the production handoff.\n");
        output
    }
}

pub(crate) fn visible(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character
                if character.is_control()
                    || matches!(
                        character,
                        '\u{061c}'
                            | '\u{200e}'
                            | '\u{200f}'
                            | '\u{2028}'
                            | '\u{2029}'
                            | '\u{202a}'..='\u{202e}'
                            | '\u{2066}'..='\u{2069}'
                    ) =>
            {
                use std::fmt::Write as _;
                let _result = write!(escaped, "\\u{{{:04x}}}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    escaped
}

#[derive(Debug, Serialize)]
pub(crate) struct Profile {
    pub(crate) name: &'static str,
    pub(crate) version: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct Source {
    pub(crate) json: String,
    pub(crate) atlas: String,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct Summary {
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct Readiness {
    pub(crate) required_animations: usize,
    pub(crate) hats: usize,
    pub(crate) collars: usize,
    pub(crate) glasses: usize,
    pub(crate) third_glasses_skin: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Inventory {
    pub(crate) bones: usize,
    pub(crate) slots: usize,
    pub(crate) skins: usize,
    pub(crate) attachments: usize,
    pub(crate) regions: usize,
    pub(crate) weighted_meshes: usize,
    pub(crate) unweighted_meshes: usize,
    pub(crate) linked_meshes: usize,
    pub(crate) mesh_vertices: usize,
    pub(crate) mesh_influences: usize,
    pub(crate) ik_constraints: usize,
    pub(crate) transform_constraints: usize,
    pub(crate) atlas_pages: usize,
    pub(crate) atlas_regions: usize,
    pub(crate) animations: Vec<AnimationInventory>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AnimationInventory {
    pub(crate) name: String,
    pub(crate) duration_seconds: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct Finding {
    pub(crate) severity: String,
    pub(crate) code: String,
    pub(crate) scope: String,
    pub(crate) message: String,
    pub(crate) fix: String,
}

impl Finding {
    fn new(
        severity: &str,
        code: impl Into<String>,
        scope: impl Into<String>,
        message: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self {
            severity: severity.to_owned(),
            code: code.into(),
            scope: scope.into(),
            message: message.into(),
            fix: fix.into(),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Unverified {
    pub(crate) setting: &'static str,
    pub(crate) reason: &'static str,
}

fn unverified_settings() -> Vec<Unverified> {
    [
        ("bleed", "not encoded in JSON, atlas, or PNG metadata"),
        ("padding-and-edge-padding", "not encoded in the final atlas"),
        (
            "strip-whitespace",
            "trimmed output does not prove the packer toggle",
        ),
        (
            "animation-clean-up",
            "the export does not retain the editor toggle",
        ),
        (
            "nonessential-data",
            "absence cannot prove the export toggle",
        ),
        (
            "editor-export-warnings",
            "warnings are not embedded in the export",
        ),
        (
            "fully-coloured-artwork",
            "artistic intent is not machine-verifiable",
        ),
    ]
    .into_iter()
    .map(|(setting, reason)| Unverified { setting, reason })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_fields_make_terminal_controls_visible() {
        assert_eq!(visible("cat\n\u{1b}[2J.json"), "cat\\n\\u{001b}[2J.json");
        assert_eq!(visible("safe\tfield"), "safe\\tfield");
        assert_eq!(
            visible("a\u{061c}\u{200e}\u{200f}\u{202a}\u{2069}z"),
            "a\\u{061c}\\u{200e}\\u{200f}\\u{202a}\\u{2069}z"
        );
    }
}

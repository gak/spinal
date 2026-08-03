//! Repository-only consistency checks for the fixture evidence ledger.

use std::collections::HashSet;

const COVERAGE: &str = include_str!("../../fixtures/COVERAGE.toml");

#[test]
fn fixture_coverage_references_declared_evidence_and_production_fixtures() {
    let fixture_ids = section_values("fixtures", "id");
    let coverage_ids = section_values("coverage", "id");

    assert_unique("fixture", &fixture_ids);
    assert_unique("coverage", &coverage_ids);
    assert!(
        !fixture_ids.is_empty(),
        "the fixture ledger must not be empty"
    );
    assert!(
        !coverage_ids.is_empty(),
        "the coverage matrix must not be empty"
    );

    for evidence in all_array_values("implementation_evidence") {
        assert!(
            fixture_ids.contains(&evidence),
            "implementation_evidence references undeclared fixture `{evidence}`"
        );
    }
    for production_fixture in all_scalar_values("production_fixture") {
        assert!(
            fixture_ids.contains(&production_fixture),
            "production_fixture references undeclared fixture `{production_fixture}`"
        );
    }
}

fn section_values(section: &str, key: &str) -> Vec<String> {
    let heading = format!("[[{section}]]");
    COVERAGE
        .split(&heading)
        .skip(1)
        .filter_map(|record| {
            record
                .lines()
                .take_while(|line| !line.starts_with("[["))
                .find_map(|line| scalar_value(line, key))
        })
        .collect()
}

fn all_scalar_values(key: &str) -> Vec<String> {
    COVERAGE
        .lines()
        .filter_map(|line| scalar_value(line, key))
        .collect()
}

fn scalar_value(line: &str, key: &str) -> Option<String> {
    let value = line.trim().strip_prefix(key)?.trim_start();
    let value = value.strip_prefix('=')?.trim();
    value
        .strip_prefix('"')?
        .strip_suffix('"')
        .map(ToOwned::to_owned)
}

fn all_array_values(key: &str) -> Vec<String> {
    COVERAGE
        .lines()
        .filter_map(|line| {
            let value = line.trim().strip_prefix(key)?.trim_start();
            value
                .strip_prefix('=')?
                .trim()
                .strip_prefix('[')?
                .strip_suffix(']')
        })
        .flat_map(|values| {
            values.split(',').filter_map(|value| {
                value
                    .trim()
                    .strip_prefix('"')?
                    .strip_suffix('"')
                    .map(ToOwned::to_owned)
            })
        })
        .collect()
}

fn assert_unique(kind: &str, values: &[String]) {
    let mut unique = HashSet::new();
    for value in values {
        assert!(unique.insert(value), "duplicate {kind} id `{value}`");
    }
}

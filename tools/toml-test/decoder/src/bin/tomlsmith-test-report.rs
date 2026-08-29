use std::{collections::HashSet, path::Path};

use serde_json::Value;

const EXPECTED_RUNNER_VERSION: &str = "toml-test v2.2.0";

fn main() {
    let reports = std::env::args().skip(1).collect::<Vec<_>>();
    if reports.is_empty() {
        eprintln!("usage: tomlsmith-test-report <report.json>...");
        std::process::exit(2);
    }

    let mut rejected = false;
    let mut seen_versions = HashSet::new();
    for report in reports {
        match verify(Path::new(&report)) {
            Ok(toml) if seen_versions.insert(toml) => {}
            Ok(toml) => {
                eprintln!("duplicate TOML {toml} report");
                rejected = true;
            }
            Err(error) => {
                eprintln!("{error}");
                rejected = true;
            }
        }
    }
    for required in ["1.0.0", "1.1.0"] {
        if !seen_versions.contains(required) {
            eprintln!("missing TOML {required} report");
            rejected = true;
        }
    }
    if rejected {
        std::process::exit(1);
    }
}

fn verify(path: &Path) -> Result<&'static str, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let report: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to decode {}: {error}", path.display()))?;
    let runner_version = report
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{} has no string `version` field", path.display()))?;
    if runner_version != EXPECTED_RUNNER_VERSION {
        return Err(format!(
            "{} was produced by {runner_version:?}, expected {EXPECTED_RUNNER_VERSION:?}",
            path.display()
        ));
    }
    let toml = report
        .get("toml")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{} has no string `toml` field", path.display()))?;
    let (toml, expected_valid, expected_invalid) = match toml {
        "1.0.0" => ("1.0.0", 205, 474),
        "1.1.0" => ("1.1.0", 214, 467),
        _ => {
            return Err(format!(
                "{} has unexpected TOML version {toml:?}",
                path.display()
            ));
        }
    };
    let passed_valid = count(&report, path, "passed_valid")?;
    let failed_valid = count(&report, path, "failed_valid")?;
    let passed_invalid = count(&report, path, "passed_invalid")?;
    let failed_invalid = count(&report, path, "failed_invalid")?;
    let passed_encoder = count(&report, path, "passed_encoder")?;
    let failed_encoder = count(&report, path, "failed_encoder")?;
    let skipped = count(&report, path, "skipped")?;

    if failed_valid != 0
        || failed_invalid != 0
        || passed_encoder != 0
        || failed_encoder != 0
        || skipped != 0
    {
        return Err(format!(
            "{} ({toml}) is not a strict pass: failed_valid={failed_valid}, \
             failed_invalid={failed_invalid}, passed_encoder={passed_encoder}, \
             failed_encoder={failed_encoder}, skipped={skipped}",
            path.display()
        ));
    }
    if passed_valid != expected_valid || passed_invalid != expected_invalid {
        return Err(format!(
            "{} ({toml}) ran valid={passed_valid} and invalid={passed_invalid}; \
             expected {expected_valid} valid and {expected_invalid} invalid",
            path.display()
        ));
    }
    println!(
        "{} ({toml}): valid={passed_valid}, invalid={passed_invalid}, failed=0, skipped=0",
        path.display()
    );
    Ok(toml)
}

fn count(report: &Value, path: &Path, field: &str) -> Result<u64, String> {
    report
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{} has no unsigned integer `{field}` field", path.display()))
}

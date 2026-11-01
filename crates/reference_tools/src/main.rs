//! Rules-fixture runner. External-engine adapters and whole-card execution are not implemented.
use serde::{Deserialize, Serialize};
use spellwake_contracts::{ApiResponse, RulesInteger};
use spellwake_engine::{Allocation, DistributionDecision, TargetId, validate_distribution};
use std::{collections::BTreeSet, error::Error, process::ExitCode};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Suite {
    version: u32,
    source: String,
    rule: String,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    name: String,
    revision: u64,
    expected_revision: u64,
    total: RulesInteger,
    minimum_targets: usize,
    maximum_targets: usize,
    eligible_targets: Vec<u32>,
    allocations: Vec<Choice>,
    expected: Expected,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    target: u32,
    amount: RulesInteger,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Expected {
    Valid,
    Rejected { code: String },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    comparison: &'static str,
    source: String,
    rule: String,
    passed: usize,
    failed: usize,
    external_engines_compared: usize,
    fully_implemented_cards: usize,
    cases: Vec<Observation>,
}
#[derive(Serialize)]
struct Observation {
    name: String,
    passed: bool,
    actual: &'static str,
}

fn run() -> Result<Report, Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let input = if let Some(path) = arguments.next() {
        std::fs::read_to_string(path)?
    } else {
        include_str!("../../../tests/scenarios/distributed-damage.json").into()
    };
    if arguments.next().is_some() {
        return Err("expected zero or one scenario path".into());
    }
    evaluate(&input)
}

fn evaluate(input: &str) -> Result<Report, Box<dyn Error>> {
    let suite: Suite = serde_json::from_str(input)?;
    if suite.version != 1 {
        return Err("unsupported fixture contract version".into());
    }
    if suite.cases.is_empty() || suite.source.is_empty() || suite.rule.is_empty() {
        return Err("fixture requires source, rule and at least one case".into());
    }
    let mut names = BTreeSet::new();
    let mut observations = Vec::new();
    for case in suite.cases {
        if case.name.is_empty() || !names.insert(case.name.clone()) {
            return Err("fixture case names must be nonempty and unique".into());
        }
        let decision = DistributionDecision {
            revision: case.revision,
            total: case.total,
            minimum_targets: case.minimum_targets,
            maximum_targets: case.maximum_targets,
            eligible_targets: case.eligible_targets.into_iter().map(TargetId).collect(),
        };
        let actual = match validate_distribution(
            &decision,
            case.expected_revision,
            case.allocations
                .into_iter()
                .map(|choice| Allocation {
                    target: TargetId(choice.target),
                    amount: choice.amount,
                })
                .collect(),
        ) {
            Ok(_) => "valid",
            Err(error) => error.code(),
        };
        let passed = match case.expected {
            Expected::Valid => actual == "valid",
            Expected::Rejected { code } => actual != "valid" && actual == code,
        };
        observations.push(Observation {
            name: case.name,
            passed,
            actual,
        });
    }
    let passed = observations.iter().filter(|case| case.passed).count();
    Ok(Report {
        comparison: "rules-fixture",
        source: suite.source,
        rule: suite.rule,
        passed,
        failed: observations.len() - passed,
        external_engines_compared: 0,
        fully_implemented_cards: 0,
        cases: observations,
    })
}

fn main() -> ExitCode {
    let (response, exit) = match run() {
        Ok(report) => {
            let exit = if report.failed == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
            (ApiResponse::<Report>::Success { data: report }, exit)
        }
        Err(error) => {
            eprintln!("reference fixture error: {error}");
            let response = if error.downcast_ref::<std::io::Error>().is_some() {
                ApiResponse::Error {
                    message: "Could not read scenario fixture".into(),
                    code: "fixture-io-error".into(),
                }
            } else {
                ApiResponse::Fail {
                    data: std::collections::BTreeMap::from([("fixture".into(), error.to_string())]),
                }
            };
            (response, ExitCode::FAILURE)
        }
    };
    match serde_json::to_string_pretty(&response) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("report serialization error: {error}");
            return ExitCode::FAILURE;
        }
    }
    exit
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &str = include_str!("../../../tests/scenarios/distributed-damage.json");

    #[test]
    fn runs_all_sourced_mechanism_cases_without_claiming_engine_comparisons() {
        let report = evaluate(FIXTURE).unwrap();
        assert_eq!(report.passed, 13);
        assert_eq!(report.failed, 0);
        assert_eq!(report.external_engines_compared, 0);
        assert_eq!(report.fully_implemented_cards, 0);
    }

    #[test]
    fn detects_an_incorrect_expected_observation() {
        let mut fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        fixture["cases"][0]["expected"] =
            serde_json::json!({"kind":"rejected","code":"incorrect-total"});
        let report = evaluate(&fixture.to_string()).unwrap();
        assert_eq!(report.failed, 1);
        assert!(!report.cases[0].passed);
    }

    #[test]
    fn rejects_unknown_versions_empty_suites_and_duplicate_case_names() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let mut unsupported = fixture.clone();
        unsupported["version"] = 2.into();
        assert!(evaluate(&unsupported.to_string()).is_err());
        let mut empty = fixture.clone();
        empty["cases"] = serde_json::json!([]);
        assert!(evaluate(&empty.to_string()).is_err());
        let mut duplicate = fixture;
        duplicate["cases"][1]["name"] = duplicate["cases"][0]["name"].clone();
        assert!(evaluate(&duplicate.to_string()).is_err());
    }
}

#![cfg(feature = "converter")]

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

fn matrix() -> Value {
    let raw = fs::read_to_string("migration-capability-matrix.json")
        .expect("read migration-capability-matrix.json");
    serde_json::from_str(&raw).expect("parse migration-capability-matrix.json")
}

#[test]
fn migration_capability_matrix_has_valid_shape_and_unique_ids() {
    let matrix = matrix();
    assert_eq!(
        matrix.get("schema_version").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        matrix.pointer("/policy/canonical").and_then(Value::as_bool),
        Some(true)
    );

    let allowed_statuses = matrix
        .get("status_values")
        .and_then(Value::as_array)
        .expect("status_values array")
        .iter()
        .map(|value| value.as_str().expect("status value").to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        allowed_statuses,
        ["blocked", "partial", "supported", "unknown"]
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
    );

    let features = matrix
        .get("features")
        .and_then(Value::as_array)
        .expect("features array");
    assert!(
        features.len() >= 100,
        "matrix should cover the broad COBOL migration surface, got {} features",
        features.len()
    );

    let mut ids = BTreeSet::new();
    let mut categories = BTreeSet::new();
    for feature in features {
        let id = feature
            .get("id")
            .and_then(Value::as_str)
            .expect("feature id");
        assert!(
            ids.insert(id.to_string()),
            "duplicate feature id in capability matrix: {id}"
        );
        assert!(
            id.contains('.'),
            "feature id should be category-qualified: {id}"
        );

        let category = feature
            .get("category")
            .and_then(Value::as_str)
            .expect("feature category");
        categories.insert(category.to_string());

        let name = feature
            .get("name")
            .and_then(Value::as_str)
            .expect("feature name");
        assert!(!name.trim().is_empty(), "feature {id} has an empty name");

        let status = feature
            .get("status")
            .and_then(Value::as_str)
            .expect("feature status");
        assert!(
            allowed_statuses.contains(status),
            "feature {id} has invalid status {status}"
        );

        let evidence = feature
            .get("evidence")
            .and_then(Value::as_array)
            .expect("feature evidence");
        assert!(!evidence.is_empty(), "feature {id} has no evidence entries");
        for entry in evidence {
            assert!(
                entry.as_str().is_some_and(|value| !value.trim().is_empty()),
                "feature {id} has an invalid evidence entry"
            );
        }
    }

    for required in [
        "source",
        "syntax",
        "data",
        "usage",
        "file",
        "procedure",
        "condition",
        "environment",
        "dialect",
        "platform",
        "runtime",
    ] {
        assert!(
            categories.contains(required),
            "matrix is missing required category {required}"
        );
    }
}

#[test]
fn migration_capability_docs_are_synced_to_matrix_counts() {
    let matrix = matrix();
    let features = matrix
        .get("features")
        .and_then(Value::as_array)
        .expect("features array");
    let mut counts = BTreeMap::<String, usize>::new();
    for feature in features {
        let status = feature
            .get("status")
            .and_then(Value::as_str)
            .expect("feature status");
        *counts.entry(status.to_string()).or_default() += 1;
    }

    let docs = fs::read_to_string("docs/migration-capability-matrix.md").expect("read matrix docs");
    assert!(
        docs.contains("migration-capability-matrix.json"),
        "matrix docs must name the canonical JSON file"
    );
    for status in ["supported", "partial", "blocked", "unknown"] {
        let expected = format!(
            "| {status} | {} |",
            counts.get(status).copied().unwrap_or(0)
        );
        assert!(
            docs.contains(&expected),
            "matrix docs missing synced row {expected:?}"
        );
    }

    for doc_path in [
        "docs/converter.md",
        "docs/feature-map.md",
        "docs/oracle_validation.md",
    ] {
        let doc = fs::read_to_string(doc_path).expect("read doc");
        assert!(
            doc.contains("migration-capability-matrix.json"),
            "{doc_path} must reference the canonical capability matrix"
        );
    }
}

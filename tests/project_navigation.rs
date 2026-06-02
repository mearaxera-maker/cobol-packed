#![cfg(feature = "converter")]

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

fn navigation() -> Value {
    let raw = fs::read_to_string("project-navigation.json").expect("read project-navigation.json");
    serde_json::from_str(&raw).expect("parse project-navigation.json")
}

fn string_field<'a>(value: &'a Value, field: &str, context: &str) -> &'a str {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{context} missing string field {field}"))
}

fn string_array(value: &Value, field: &str, context: &str) -> Vec<String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{context} missing array field {field}"))
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .unwrap_or_else(|| panic!("{context}.{field} contains a non-string"))
                .to_string()
        })
        .collect()
}

fn assert_path_exists(path: &str, context: &str) {
    assert!(
        Path::new(path).exists(),
        "{context} references missing path {path}"
    );
}

#[test]
fn project_navigation_has_valid_shape_and_existing_paths() {
    let nav = navigation();
    assert_eq!(nav.get("schema_version").and_then(Value::as_u64), Some(1));
    assert_eq!(nav.get("canonical").and_then(Value::as_bool), Some(true));

    let start_here = nav
        .get("start_here")
        .and_then(Value::as_array)
        .expect("start_here array");
    assert!(
        start_here.len() >= 4,
        "navigation should expose the core entry points"
    );
    for entry in start_here {
        let label = string_field(entry, "label", "start_here entry");
        let path = string_field(entry, "path", label);
        assert_path_exists(path, label);
        assert!(
            !string_field(entry, "use_when", label).trim().is_empty(),
            "{label} should explain when to use it"
        );
    }

    let domains = nav
        .get("domains")
        .and_then(Value::as_array)
        .expect("domains array");
    assert!(domains.len() >= 8, "navigation should cover major domains");

    let mut domain_ids = BTreeSet::new();
    for domain in domains {
        let id = string_field(domain, "id", "domain");
        assert!(
            domain_ids.insert(id.to_string()),
            "duplicate navigation domain {id}"
        );
        assert!(
            !string_field(domain, "summary", id).trim().is_empty(),
            "domain {id} should have a summary"
        );
        let paths = string_array(domain, "primary_paths", id);
        assert!(!paths.is_empty(), "domain {id} should have primary paths");
        for path in paths {
            assert_path_exists(&path, id);
        }
    }

    let quick_lookup = nav
        .get("quick_lookup")
        .and_then(Value::as_array)
        .expect("quick_lookup array");
    assert!(
        quick_lookup.len() >= 6,
        "navigation should answer common lookup questions"
    );
    for lookup in quick_lookup {
        let question = string_field(lookup, "question", "quick_lookup entry");
        for path in string_array(lookup, "answer_paths", question) {
            assert_path_exists(&path, question);
        }
    }
}

#[test]
fn project_navigation_docs_are_wired_from_readme() {
    let doc =
        fs::read_to_string("docs/project-navigation.md").expect("read docs/project-navigation.md");
    assert!(
        doc.contains("project-navigation.json"),
        "project navigation doc must reference canonical JSON"
    );

    let readme = fs::read_to_string("README.md").expect("read README.md");
    assert!(
        readme.contains("docs/project-navigation.md"),
        "README should link the project navigation doc"
    );
}

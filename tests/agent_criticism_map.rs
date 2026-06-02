#![cfg(feature = "converter")]

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

fn json_file(path: &str) -> Value {
    let raw = fs::read_to_string(path).unwrap_or_else(|err| panic!("read {path}: {err}"));
    serde_json::from_str(&raw).unwrap_or_else(|err| panic!("parse {path}: {err}"))
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

#[test]
fn agent_criticism_map_has_valid_shape_and_references() {
    let map = json_file("agent-criticism-map.json");
    let capability = json_file("migration-capability-matrix.json");

    assert_eq!(map.get("schema_version").and_then(Value::as_u64), Some(1));
    assert_eq!(map.get("canonical").and_then(Value::as_bool), Some(true));

    let capability_ids = capability
        .get("features")
        .and_then(Value::as_array)
        .expect("capability features")
        .iter()
        .map(|feature| string_field(feature, "id", "capability feature").to_string())
        .collect::<BTreeSet<_>>();

    let lanes = map
        .get("agent_lanes")
        .and_then(Value::as_array)
        .expect("agent_lanes array");
    assert!(lanes.len() >= 8, "expected broad agent lane coverage");

    let mut lane_ids = BTreeSet::new();
    for lane in lanes {
        let id = string_field(lane, "id", "agent lane");
        assert!(lane_ids.insert(id.to_string()), "duplicate lane id {id}");
        assert!(
            !string_array(lane, "default_validations", id).is_empty(),
            "lane {id} must define validation commands"
        );
        assert!(
            !string_array(lane, "forbidden_shortcuts", id).is_empty(),
            "lane {id} must define forbidden shortcuts"
        );
    }

    let areas = map
        .get("areas")
        .and_then(Value::as_array)
        .expect("areas array");
    assert!(
        areas.len() >= 9,
        "expected dashboard coverage for all major areas"
    );

    let mut area_ids = BTreeSet::new();
    for area in areas {
        let id = string_field(area, "id", "area");
        assert!(area_ids.insert(id.to_string()), "duplicate area id {id}");
        let owner = string_field(area, "owner_lane", id);
        assert!(
            lane_ids.contains(owner),
            "area {id} has unknown owner lane {owner}"
        );

        for critic in string_array(area, "critic_lanes", id) {
            assert!(
                lane_ids.contains(&critic),
                "area {id} has unknown critic lane {critic}"
            );
            assert_ne!(critic, owner, "area {id} critic duplicates owner lane");
        }

        let estimate = area
            .get("estimate_percent")
            .unwrap_or_else(|| panic!("area {id} missing estimate_percent"));
        let min = estimate
            .get("min")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| panic!("area {id} missing estimate_percent.min"));
        let max = estimate
            .get("max")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| panic!("area {id} missing estimate_percent.max"));
        assert!(
            min <= max && max <= 100,
            "area {id} has invalid estimate range"
        );

        for capability_ref in string_array(area, "capability_refs", id) {
            assert!(
                capability_ids.contains(&capability_ref),
                "area {id} references missing capability {capability_ref}"
            );
        }
    }

    let tasks = map
        .get("tasks")
        .and_then(Value::as_array)
        .expect("tasks array");
    assert!(tasks.len() >= 5, "expected prioritized task queue");

    let mut task_ids = BTreeSet::new();
    let mut priorities = BTreeMap::<u64, String>::new();
    for task in tasks {
        let id = string_field(task, "id", "task");
        assert!(task_ids.insert(id.to_string()), "duplicate task id {id}");
        let area = string_field(task, "area", id);
        assert!(area_ids.contains(area), "task {id} has unknown area {area}");

        let owner = string_field(task, "owner_lane", id);
        assert!(
            lane_ids.contains(owner),
            "task {id} has unknown owner lane {owner}"
        );

        let critics = string_array(task, "critic_lanes", id);
        assert!(
            !critics.is_empty(),
            "task {id} must have at least one critic lane"
        );
        for critic in critics {
            assert!(
                lane_ids.contains(&critic),
                "task {id} has unknown critic lane {critic}"
            );
            assert_ne!(critic, owner, "task {id} critic duplicates owner lane");
        }

        let priority = task
            .get("priority")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| panic!("task {id} missing priority"));
        assert!(
            priorities.insert(priority, id.to_string()).is_none(),
            "duplicate task priority {priority}"
        );

        for field in ["split_when", "acceptance", "validation_commands"] {
            assert!(
                !string_array(task, field, id).is_empty(),
                "task {id} must define non-empty {field}"
            );
        }

        for capability_ref in string_array(task, "capability_refs", id) {
            assert!(
                capability_ids.contains(&capability_ref),
                "task {id} references missing capability {capability_ref}"
            );
        }
    }
}

#[test]
fn agent_dashboard_doc_references_canonical_map() {
    let doc = fs::read_to_string("docs/agent-criticism-dashboard.md")
        .expect("read docs/agent-criticism-dashboard.md");
    assert!(
        doc.contains("agent-criticism-map.json"),
        "dashboard doc must reference canonical JSON map"
    );
    assert!(
        doc.contains("migration-capability-matrix.json"),
        "dashboard doc must reference capability matrix"
    );
}

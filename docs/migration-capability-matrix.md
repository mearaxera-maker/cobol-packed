# Migration Capability Matrix

`migration-capability-matrix.json` is the canonical feature-status inventory for
the converter, VM, record tooling, and ABYSS hazard surface. Documentation should
summarize this file instead of maintaining separate, drifting support lists.

Current matrix summary:

| Status | Count |
| --- | ---: |
| supported | 10 |
| partial | 74 |
| blocked | 35 |
| unknown | 4 |

Status meanings are defined in the JSON file:

- `supported`: executable or validated for the documented subset, with
  regression evidence.
- `partial`: some forms are executable or validated, but important COBOL forms
  remain blocked or under-tested.
- `blocked`: detected and rejected before generated Rust is emitted, or
  intentionally unavailable.
- `unknown`: not yet assessed with enough source, semantic, runtime, or oracle
  evidence.

## Maintenance Rule

When a feature implementation, blocker, oracle, or ABYSS fixture changes, update
`migration-capability-matrix.json` in the same change. The test suite validates
that this document references the canonical matrix and that the status counts
above match the JSON.

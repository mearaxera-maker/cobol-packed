# Project Navigation

`project-navigation.json` is the canonical minimal map for this repository. It
exists so humans and agents can find the right files without physically
reshuffling crates, tests, or generated artifacts.

## Start Here

| Need | Go to |
| --- | --- |
| Public overview and quickstart | `README.md` |
| Supported/partial/blocked status | `migration-capability-matrix.json` |
| Maintainer roadmap, review lanes, next tasks | `agent-criticism-map.json` |
| Converter architecture and boundaries | `docs/converter.md` |
| Oracle validation model | `docs/oracle_validation.md` |

## Main Areas

| Area | Primary files |
| --- | --- |
| COMP-3 core | `src/lib.rs`, `src/cli`, `src/bin/cobol-packed.rs`, `tests/cli_smoke.rs` |
| Record layout | `crates/cobol-record`, `src/cli/record.rs` |
| Source and COPY | `crates/cobol-text`, `crates/cobol-source` |
| Parser | `crates/cobol-syntax` |
| Sema and IR | `crates/cobol-sema`, `crates/cobol-ir` |
| Codegen and VM | `crates/cobol-codegen-rust`, `crates/cobol-vm` |
| Runtime platform | `crates/cobol-platform`, `crates/cobol-runtime` |
| Converter tests | `tests/converter_smoke.rs`, `tests/converter_abyss_matrix.rs`, `tests/oracle_gnucobol.rs` |
| Maintainer dashboards | `migration-capability-matrix.json`, `agent-criticism-map.json` |
| Security | `SECURITY.md`, `security-hardening`, `docs/security` |

## How To Work

Do not move workspace crates unless the task is explicitly a workspace
restructure. Rust paths, vendored generated projects, tests, and docs already
depend on the current layout.

For implementation work:

1. Check `migration-capability-matrix.json` for feature status.
2. Check `agent-criticism-map.json` for owner and review lanes.
3. Change the smallest responsible layer.
4. Update the matrix/dashboard when status or ownership changes.
5. Run the focused tests named by the relevant dashboard entry.

## What To Ignore First

These directories are not first-stop navigation targets:

- `target`
- `proptest-regressions`
- `subprojects`

They may contain useful generated or historical artifacts, but they should not
drive the architecture.

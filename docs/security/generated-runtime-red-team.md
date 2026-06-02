# Generated Runtime Security Red-Team Pass

Scope: generated file/path behavior and dynamic CALL/runtime dispatch.

Date: 2026-05-26

## Threat Model

The repository is a local CLI/converter and generated Rust runtime for COBOL
record processing and migration previews. Primary trust boundaries:

- COBOL source, copybooks, schemas, record files, generated `cobol-file-map.json`,
  and CLI flags can be attacker-controlled when the tool is exposed through CI,
  migration automation, or batch conversion services.
- Generated Rust programs execute with the OS permissions of the operator or
  automation account.
- Dynamic COBOL `CALL` data items are program data, but the VM should only
  dispatch to linked generated programs unless separately loaded dynamic linking
  is intentionally added.

## Findings

### F1: User-Controlled Output Directory Can Remove Project Files

Affected locations:

- `crates/cobol-codegen-rust/src/lib.rs` `convert` creates the requested output
  directory before validation.
- `crates/cobol-codegen-rust/src/lib.rs` `cleanup_generated_artifacts` removes
  `src`, `vendor`, `Cargo.toml`, and `Cargo.lock` under `--out` when migration is
  blocked.

Assessment: security-relevant in automation that derives `--out` from untrusted
input or reuses a valuable repository directory as output. A malicious or
mistaken job could point `--out` at an existing project and trigger a blocker,
causing deterministic deletion of those paths. This is not remotely exploitable
by the standalone CLI unless an attacker controls CLI arguments.

Recommended control: require `--out` to be empty, absent, or contain a converter
sentinel file before cleanup; otherwise fail closed with an explicit error.

### F2: Generated File Maps Allow Arbitrary OS File Remapping

Affected locations:

- Generated `src/main.rs` auto-loads `cobol-file-map.json` from the current
  working directory or accepts `--file-map`.
- Generated `apply_file_map` deserializes a `BTreeMap<String, String>` and passes
  every value to `Program::map_file`.
- `cobol-vm` `map_external_name` replaces file backing paths, and `OPEN OUTPUT`
  uses truncating `OpenOptions`.

Assessment: intended functionality for local migration runs, but a real security
risk if generated binaries are executed in an untrusted working directory or with
an untrusted `--file-map`. The impact is arbitrary read/write/truncate within
the generated program's OS permissions for COBOL files that the program opens.

Recommended control: add an optional generated-runtime file sandbox with a base
directory allowlist. Disable implicit current-directory `cobol-file-map.json`
loading in hardened mode, or require an explicit `--file-map`.

### F3: Dynamic CALL Does Not Load Arbitrary Code

Affected locations:

- `cobol-vm` `resolve_call_target` trims literal/dynamic names.
- `cobol-vm` CALL execution checks the in-memory registered-program table.
- Missing dynamic targets set `PROGRAM-STATUS` and continue; missing literal
  targets error.

Assessment: no arbitrary dynamic library, executable, or source loading was
found in the dynamic CALL path. Current behavior is constrained to linked
generated programs. This is a positive control to preserve when separately
loaded runtime programs are introduced later.

Recommended control: keep dynamic loading behind an explicit feature/runtime
policy. If external program loading is added, enforce a program search path
allowlist and reject path separators in COBOL dynamic program names by default.

## Validation Summary

This pass used static tracing of generated project emission, generated
`main.rs`, VM file remapping/open/write behavior, and VM dynamic CALL dispatch.
No network or privileged runtime reproduction was required. The file-map issues
are configuration/embedding risks rather than vulnerabilities in the standalone
CLI default threat model, but they are reportable hardening items for migration
automation.

# Maintainer Readiness Dashboard

`agent-criticism-map.json` is the canonical machine-readable roadmap for
splitting COBOL converter work across implementation and independent review
lanes. It complements `migration-capability-matrix.json`: the capability matrix
says what the system can do; the readiness map says which risks are next, who
owns them, and what evidence is required before merge.

## Current Readiness View

| Area | Estimate | Primary lane | Main review point |
| --- | ---: | --- | --- |
| COMP-3 packed decimal core | 75-80% | codec-agent | Strong core codec; keep one sign/strictness policy. |
| COMP-3 inside generated COBOL programs | 60-65% | codegen-vm-agent | Accessors exist; arithmetic/sort/comparison coverage is narrower. |
| Source and COPY normalization | 45-50% | source-agent | Source corruption before parsing invalidates downstream results. |
| Syntax and procedure parsing | 50-55% | syntax-agent | Token-slice parsing helped; full sentence CFG is still missing. |
| Semantic analysis and IR | 45-50% | sema-ir-agent | Aliasing, ODO, REDEFINES, RENAMES, and raw IR escape hatches remain high risk. |
| Codegen and VM | 45-50% | codegen-vm-agent | VM surface is growing; platform semantics and checkpoint interactions remain partial. |
| File and OS runtime | 25-35% | runtime-file-agent | Fixed sequential works; VSAM/indexed/relative/tape/locking are not real yet. |
| Oracle validation | 20-25% | oracle-agent | Seed oracle only; not a migration certification suite. |
| Full mainframe and ABYSS compatibility | 25-30% | abyss-agent | Some hazards exist; overlays, tape, RERUN depth, and EBCDIC platform behavior remain open. |

## Split Rule

Use one implementation owner per task. Add at least one spec review lane and
one edge-case review lane. Split work only when the lanes can validate
independently: parser vs oracle fixtures, codec vs generated-program
integration, runtime behavior vs security/red-team probes.

Do not split a cross-cutting AST/IR contract across competing implementers. The
syntax/sema/codegen boundary must have one owner until the contract is stable.

## Priority Queue

1. `sentence-cfg-v1`: implement sentence CFG and real `NEXT SENTENCE` semantics.
2. `packed-program-arithmetic-v1`: broaden generated packed-decimal arithmetic and comparisons.
3. `preprocessor-source-map-v1`: build token/source-map-aware fixed/COPY preprocessing.
4. `file-runtime-platform-v1`: harden OS file/runtime platform behavior.
5. `abyss-matrix-one-hazard-fixtures`: make ABYSS diagnostics exact and one-hazard-per-fixture.

## Required Evidence

Every task in `agent-criticism-map.json` must name:

- owner lane;
- critic lanes;
- capability matrix references;
- split decision;
- acceptance criteria;
- validation commands.

The test suite validates this shape so the dashboard cannot silently drift into
an unreviewable task list.

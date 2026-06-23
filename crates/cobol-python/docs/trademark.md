# Trademark And Naming Review

This document is an engineering release gate, not legal advice. A human legal or
business reviewer must complete trademark clearance before any public
redistribution that uses a product or service name.

## Current Project Name

- Repository/package name: `cobol_packed`
- CLI binary names: `cobol-packed`, `cobol2rust`
- Python package name: `cobol-converter`

No file in this repository grants trademark rights in COBOL, GnuCOBOL, IBM,
Micro Focus, Microsoft, Linux, macOS, Windows, or any other third-party mark.
Those names are used descriptively for compatibility, platform, or oracle-test
documentation.

## Release Gate

Before publishing a public release, record the reviewer, date, scope, and result
of the name search in the release issue or approval record. At minimum, review:

- package names and binary names
- README, docs, release notes, and archive names
- Docker image names and labels if publishing an image
- PyPI/crates.io/GitHub Release titles
- downstream customer-facing product names

If a name is not cleared, rename before public redistribution or obtain written
approval for the intended descriptive use.

## Compatibility Wording

Use compatibility language such as "COBOL-to-Rust converter", "GnuCOBOL oracle
validation", and "IBM Enterprise COBOL data encoding support". Do not imply
endorsement, certification, partnership, or official status with any third-party
vendor or project unless a separate written agreement exists.

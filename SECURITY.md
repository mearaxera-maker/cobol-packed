# Security Policy

## Supported Versions

Security fixes are provided for the latest published `1.x` release.

## Reporting A Vulnerability

Please report suspected vulnerabilities privately through GitHub Security
Advisories for this repository. If advisories are unavailable, email the
maintainer listed on crates.io with a short description, affected version,
reproduction steps, and whether public disclosure has already occurred.

Do not open a public issue for a vulnerability until a fix or mitigation is
available.

## Response Expectations

The project aims to acknowledge reports within 5 business days. Confirmed
vulnerabilities receive a patched release, a changelog entry, and a public
advisory once users have a reasonable upgrade path.

## Scope

In scope:

- Memory safety or panic paths reachable from untrusted input.
- Incorrect packed-decimal, EBCDIC, or schema validation behavior that can
  silently corrupt decoded data.
- CLI behavior that leaks partial records, ignores configured error policy, or
  misrepresents audit results.
- Supply-chain or release-artifact integrity issues.

Out of scope:

- General COBOL program conversion features; this crate decodes data records.
- Unsupported EBCDIC codepages that are rejected at schema validation time.
- Denial-of-service reports requiring inputs beyond the documented limits.

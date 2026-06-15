# Security Policy

## Reporting Vulnerabilities

If you discover a correctness, panic-safety, overflow, SIMD divergence, or forensic-integrity issue in `cobol_packed`, please report it privately before public disclosure.

Preferred contact methods:
- Open a GitHub Security Advisory for this repository
- Or raise a private issue and request the maintainers enable private disclosure

Priority classes:
1. Scalar/SIMD divergence
2. Overflow law violations
3. Negative-zero corruption
4. Invalid nibble acceptance
5. Panic paths
6. Endianness inconsistencies

Timeline and expectations:
- Acknowledge within 72 hours of a valid report
- We aim to provide a remediation plan or CVE disposition within 14 days

If you intend to provide a patch, please include a short test case and steps to reproduce. Do not disclose publicly until a fix or mitigation is available.

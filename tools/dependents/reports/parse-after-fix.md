# Dependents report: parse

Real projects' own test suites, run against the original library and against the port. This is a report, not a gate. A test is flagged only if it passed in every run with the original and failed with the port.

- **Date:** 2026-11-27
- **Original:** https://github.com/r1chardj0n3s/parse at `7f07b64089338ee7b7db85a752eb45c4816c2657`
- **Port:** `ports/parse` at Portwright commit `87be342`, wheel `parse_rust_port-0.1.0-cp312-cp312-manylinux_2_34_x86_64.whl` (sha256 `751e336696a2911c`)
- **Python:** 3.12.14

| Dependent | Version | Tests | Pass with original | Fail with original | Flaky with original | Flagged |
| --- | --- | --- | --- | --- | --- | --- |
| parse_type | 0.6.6 (`5c4126ae39`) | 334 | 331 | 1 | 0 | 0 |
| pytest-bdd | 9.0.1 (`aad3bf4350`) | 146 | 145 | 0 | 0 | 0 |

## parse_type 0.6.6

- `original-1`: 331 passed, 2 skipped, 1 failed; pytest exit 1
- `original-2`: 331 passed, 2 skipped, 1 failed; pytest exit 1
- `port-1`: 331 passed, 2 skipped, 1 failed; pytest exit 1

Failing with the original too (not flagged): `tests.parse_tests.test_pattern::test_numbered`

Nothing flagged.

## pytest-bdd 9.0.1

- `original-1`: 145 passed, 1 skipped; pytest exit 0
- `original-2`: 145 passed, 1 skipped; pytest exit 0
- `port-1`: 145 passed, 1 skipped; pytest exit 0

Nothing flagged.

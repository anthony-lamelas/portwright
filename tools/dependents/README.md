# Dependents report

Runs real projects that use a ported library against the original and against the port, to find behavior the gates miss. It is a report, not a gate: it doesn't change a run's result, and nothing in `portwright/` reads it.

For each dependent in `<lib>.json`, `run.py` makes fresh venvs on the gates' Python, installs the dependent at a pinned commit with the exact versions in its lock file, then runs the dependent's own pytest suite twice with the original library (at the commit the port was made from) and once with the port's wheel under the same import name. A test is **flagged** only if it passed in both original runs and failed or errored with the port. Tests that fail with the original, or differ between its two runs, are listed but not flagged.

```sh
python tools/dependents/run.py --lib parse                 # builds the wheel in the toolchain image
python tools/dependents/run.py --lib parse --wheel <whl>   # or use a wheel you built
python tools/dependents/run.py --lib parse --lock          # regenerate the lock files
```

It needs git, uv, network access to PyPI and GitHub, and Docker with the toolchain image to build the wheel. Dependents' tests run on the host, unsandboxed, so list only projects whose tests you would run anyway. Before running a port's tests, it checks which installed distribution provides the import name, so a run can't silently test the original twice.

## parse (2026-11-27): 16 tests flagged, two causes

Report: [`reports/parse.md`](reports/parse.md). Dependents: **parse_type 0.6.6**, a set of extensions built on parse (331 of 334 tests pass with the original), and **pytest-bdd 9.0.1**, whose step parsers use parse and parse_type (145 of 146 pass with the original). Both original runs agreed on every test, so no flaky tests had to be set aside.

1. **`parse.PARSE_RE` doesn't exist in the port (12 tests: 9 in parse_type, 3 in pytest-bdd).** `parse_type.parse_util.FieldParser` splits formats with `parse.PARSE_RE`, the compiled regex for `{...}` fields. pytest-bdd reaches it through `parse_type.cfparse.Parser`. It isn't in parse's `__all__`, but it has no leading underscore, and a well-known dependent relies on it. The port is missing 22 non-underscore module names in all. Most are imports (`re`, `datetime`, `log`) that nobody should use, but `PARSE_RE`, `ALLOWED_TYPES`, `MONTHS_MAP` and the `*_PAT` patterns are data a dependent could read.
2. **Subclassing `parse.Parser` doesn't work (4 tests in parse_type).** The port builds a `Parser` in `__new__`, as PyO3 classes do, where the original builds it in `__init__`. A subclass that takes an extra argument (`type_builder=`), or adds types to `extra_types` before calling `super().__init__()`, fails. `parse_type.cfparse.Parser` does both. Two minimal subclasses show it: one with an extra keyword argument fails with `TypeError: Parser.__new__() got an unexpected keyword argument`, and one that rewrites the format before `super().__init__(fmt)` fails with `TypeError: object.__init__() takes exactly one argument`. Both work with the original.

**Should the gates have caught them?** Both, yes, and both can be detected without a dependent's tests:

- The comparison only calls the APIs that intake lists (`__all__` and the classes they return), so no gate compares the module's own public names. A surface check would have caught cause 1 at intake: every non-underscore, non-module attribute of the original must exist in the port, with the same kind of value. It would need an allow-list for imported names like `re` and `datetime`.
- No gate subclasses a public class. The differential calls `Parser(...)` and its methods directly, and the upstream tests never subclass it. A generic probe would have caught cause 2: for each public class, compare a subclass whose `__init__` takes an extra keyword argument, and one that changes the arguments before calling `super().__init__`.

**Fixed (2026-11-27).** Both causes are fixed by hand in `ports/parse` (see its `CONFORMANCE.md`), and the main gates pass on the result. A rerun flags nothing: [`reports/parse-after-fix.md`](reports/parse-after-fix.md). The two gate checks above are still not built.

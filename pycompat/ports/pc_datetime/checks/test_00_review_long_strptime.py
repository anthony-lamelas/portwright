"""Review stage 2, finding 1: strptime with very long formats (the port's regex crate has a
compiled-size limit; CPython has none). Sizes are chosen just above where a 64 MiB regex-crate
limit fails, measured per literal kind:
  'ι' ~200k chars, ' ι' ~64k pairs, ' k' ~71k pairs, ' a' ~84k pairs, 'k' ~250k, 'é' ~350k, 'a' ~850k.
These calls take CPython ~6 s under coverage tracing (0.5 s without), so this file is named to
run first: if the coverage gate's worker times out here it loses only this file's coverage."""
from hypothesis import given, example, settings, HealthCheck, strategies as st
from portwright.compare import same

SLOW = settings(deadline=None, max_examples=4, suppress_health_check=list(HealthCheck))

UNITS = st.sampled_from([("ι", 205_000), (" ι", 67_000), (" k", 75_000), (" a", 88_000),
                         ("\tι", 67_000), (" ſ", 75_000), ("ι-", 140_000)])
VALS = {"%Y": "2024", "%m": "03", "%d": "07", "%H": "13", "%M": "05", "%S": "09", "%b": "mar",
        "%A": "THURSDAY", "%p": "pm", "%z": "+0130", "%j": "067", "%f": "5", "%Z": "UTC"}


def build(unit, n, where, dirs):
    lit = unit * n
    fmt_dirs = "-".join(dirs)
    txt_dirs = "-".join(VALS[d] for d in dirs)
    if where == "prefix":
        return lit + txt_dirs, lit + fmt_dirs
    if where == "suffix":
        return txt_dirs + lit, fmt_dirs + lit
    half = unit * (n // 2)
    return half + txt_dirs + half, half + fmt_dirs + half


# reviewer's repros, verbatim sizes
@settings(deadline=None, max_examples=1, suppress_health_check=list(HealthCheck))
@given(st.just(("2024" + "é" * 400_000, "%Y" + "é" * 400_000)))
@example(("2024" + "é" * 400_000, "%Y" + "é" * 400_000))
@example(("2024" + "k" * 400_000, "%Y" + "k" * 400_000))
@example(("2024" + "a" * 1_000_000, "%Y" + "a" * 1_000_000))
def test_strptime_reviewer_long_formats(case):
    same("strptime", *case)


@SLOW
@given(UNITS, st.sampled_from(["prefix", "suffix", "both"]),
       st.lists(st.sampled_from(sorted(VALS)), min_size=1, max_size=4, unique=True),
       st.sampled_from(["match", "mismatch", "upper"]))
@example(("ι", 205_000), "suffix", ["%Y"], "match")
@example((" ι", 67_000), "suffix", ["%Y"], "match")
@example((" k", 75_000), "both", ["%Y", "%m", "%d"], "match")
@example((" a", 88_000), "prefix", ["%H", "%M"], "mismatch")
def test_strptime_long_literal_runs(unit_n, where, dirs, mode):
    unit, n = unit_n
    s, fmt = build(unit, n, where, dirs)
    if mode == "mismatch":
        s = s[:-1] + "#"
    elif mode == "upper":
        s = s.upper()      # case-insensitive literal matching over the long run
    same("strptime", s, fmt)

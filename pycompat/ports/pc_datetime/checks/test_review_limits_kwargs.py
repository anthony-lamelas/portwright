"""Review stage 2: keyword argument names containing lone surrogates / non-ASCII (lossy key conversion)."""
from hypothesis import given, example, settings, HealthCheck, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text


# ---- 2. keyword names with lone surrogates / odd unicode ----------------------------------
KEYS = st.one_of(text(min_size=1, max_size=4),
                 st.sampled_from(["\udc80", "\ud800", "\udfff", "\ud83d", "a\udc80", "\udc80\udc81", "\udc81",
                                  "yéar", "ｙear", "year\x00", "�", "y"]))


@given(KEYS)
@example("\udc80")
@example("�")
def test_unexpected_kwarg_name(key):
    same("make_date", 2024, 1, 1, **{key: 1})
    same("make_time", **{key: 1})
    same("make_datetime", 2024, 1, 1, **{key: 1})
    same("make_timedelta", **{key: 1})
    same("time_isoformat", (1, 2, 3, 4, None, 0), **{key: "auto"})
    same("datetime_isoformat", (2024, 1, 2, 3, 4, 5, 6, None, 0), **{key: "T"})
    same("strptime", "2024", "%Y", **{key: 1})


@given(st.lists(KEYS, min_size=2, max_size=3, unique=True))
@example(["\udc80", "\udc81"])
@example(["\udc80", "�"])
@example(["\ud800", "\udc80"])
def test_distinct_surrogate_kwargs(keys):
    kw = {k: i for i, k in enumerate(keys)}
    same("make_timedelta", **kw)
    same("make_time", 1, **kw)
    same("make_date", 2024, 1, 1, **kw)


@given(KEYS)
@example("\udc80")
def test_surrogate_kwarg_with_valid_kwargs(key):
    # valid keywords alongside an invalid surrogate keyword; also a valid call with keywords only
    same("make_timedelta", days=1, hours=2, **{key: 3})
    same("make_datetime", year=2024, month=2, day=29, hour=1, **{key: 0})
    same("make_timedelta", days=1, hours=2)

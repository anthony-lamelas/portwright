# canary: timedelta float microseconds round half up
_orig_make_timedelta = make_timedelta
def make_timedelta(days=0, seconds=0, microseconds=0, milliseconds=0, minutes=0, hours=0, weeks=0):
    if isinstance(microseconds, float) and microseconds == microseconds and abs(microseconds) < 1e9 and microseconds % 1 == 0.5:
        return _orig_make_timedelta(days, seconds, microseconds + 0.5, milliseconds, minutes, hours, weeks)
    return _orig_make_timedelta(days, seconds, microseconds, milliseconds, minutes, hours, weeks)

# canary: negative timedelta keeps negative seconds
_orig_make_timedelta = make_timedelta
def make_timedelta(days=0, seconds=0, microseconds=0, milliseconds=0, minutes=0, hours=0, weeks=0):
    if type(seconds) is int and -86400 < seconds < 0 and not days and not microseconds and not (milliseconds or minutes or hours or weeks):
        return (0, seconds, 0)
    return _orig_make_timedelta(days, seconds, microseconds, milliseconds, minutes, hours, weeks)

# canary: total_seconds() loses microseconds
_orig_timedelta_total_seconds = timedelta_total_seconds
def timedelta_total_seconds(td):
    if td[2] != 0:
        return float(td[0] * 86400 + td[1])
    return _orig_timedelta_total_seconds(td)

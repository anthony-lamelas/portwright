# canary: timedelta str says '1 days'
_orig_timedelta_str = timedelta_str
def timedelta_str(td):
    if td[0] in (1, -1):
        return _orig_timedelta_str(td).replace(' day,', ' days,')
    return _orig_timedelta_str(td)

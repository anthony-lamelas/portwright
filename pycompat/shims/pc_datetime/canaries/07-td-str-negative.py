# canary: negative timedelta str shows a minus sign
_orig_timedelta_str = timedelta_str
def timedelta_str(td):
    if td[0] < 0:
        return '-' + _orig_timedelta_str((-td[0] - 1, 86399 - td[1], 1000000 - td[2]) if td[2] else (-td[0] - 1, 86400 - td[1], 0)) if td[1] or td[2] else '-' + _orig_timedelta_str((-td[0], 0, 0))
    return _orig_timedelta_str(td)

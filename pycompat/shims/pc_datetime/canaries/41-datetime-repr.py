# canary: repr shows microsecond=0
_orig_datetime_repr = datetime_repr
def datetime_repr(d):
    if d[6] == 0 and d[5] != 0:
        return _orig_datetime_repr(d)[:-1] + ', 0)' if d[7] is None else _orig_datetime_repr(d)
    return _orig_datetime_repr(d)

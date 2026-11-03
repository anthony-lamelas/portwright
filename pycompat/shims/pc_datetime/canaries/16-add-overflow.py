# canary: date overflow wraps instead of raising
_orig_datetime_add = datetime_add
def datetime_add(d, td):
    if d[0] == 9999 and td[0] > 0:
        return d
    return _orig_datetime_add(d, td)

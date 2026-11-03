# canary: repr of zero timedelta lists days=0
_orig_timedelta_repr = timedelta_repr
def timedelta_repr(td):
    if td == (0, 0, 0):
        return 'datetime.timedelta(days=0)'
    return _orig_timedelta_repr(td)

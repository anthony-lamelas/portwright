# canary: offset seconds dropped in isoformat
_orig_datetime_isoformat = datetime_isoformat
def datetime_isoformat(d, sep='T', timespec='auto'):
    if d[7] is not None and d[7] % 60:
        return _orig_datetime_isoformat(d[:7] + (d[7] - d[7] % 60,) + d[8:], sep, timespec)
    return _orig_datetime_isoformat(d, sep, timespec)

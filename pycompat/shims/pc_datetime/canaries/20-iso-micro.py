# canary: isoformat always shows microseconds
_orig_datetime_isoformat = datetime_isoformat
def datetime_isoformat(d, sep='T', timespec='auto'):
    if timespec == 'auto' and d[6] == 0:
        return _orig_datetime_isoformat(d, sep, 'microseconds')
    return _orig_datetime_isoformat(d, sep, timespec)

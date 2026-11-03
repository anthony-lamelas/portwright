# canary: milliseconds timespec rounds
_orig_datetime_isoformat = datetime_isoformat
def datetime_isoformat(d, sep='T', timespec='auto'):
    if timespec == 'milliseconds' and d[6] % 1000 >= 500:
        return _orig_datetime_isoformat(d[:6] + (min(d[6] + 500, 999999),) + d[7:], sep, timespec)
    return _orig_datetime_isoformat(d, sep, timespec)

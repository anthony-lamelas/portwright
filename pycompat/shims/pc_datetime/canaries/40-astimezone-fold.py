# canary: astimezone keeps the input offset
_orig_datetime_astimezone = datetime_astimezone
def datetime_astimezone(d, utcoffset):
    if d[7] is not None and utcoffset != d[7]:
        return d
    return _orig_datetime_astimezone(d, utcoffset)

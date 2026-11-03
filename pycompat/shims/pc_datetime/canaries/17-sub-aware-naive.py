# canary: aware minus naive ignores the offset
_orig_datetime_subtract = datetime_subtract
def datetime_subtract(a, b):
    if (a[7] is None) != (b[7] is None):
        return _orig_datetime_subtract(a[:7] + (None, a[8]), b[:7] + (None, b[8]))
    return _orig_datetime_subtract(a, b)

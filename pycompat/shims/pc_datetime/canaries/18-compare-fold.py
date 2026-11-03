# canary: fold breaks equality
_orig_datetime_compare = datetime_compare
def datetime_compare(a, b):
    if a[:8] == b[:8] and a[8] != b[8]:
        return -1 if a[8] < b[8] else 1
    return _orig_datetime_compare(a, b)

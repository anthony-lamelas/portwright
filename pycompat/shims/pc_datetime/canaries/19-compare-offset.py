# canary: comparison ignores offsets
_orig_datetime_compare = datetime_compare
def datetime_compare(a, b):
    if a[7] is not None and b[7] is not None and a[7] != b[7]:
        return (a[:7] > b[:7]) - (a[:7] < b[:7])
    return _orig_datetime_compare(a, b)

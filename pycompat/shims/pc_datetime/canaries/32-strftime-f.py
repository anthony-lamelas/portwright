# canary: %f drops leading zeros
_orig_strftime = strftime
def strftime(d, fmt):
    if '%f' in fmt and d[6] < 100000:
        return _orig_strftime(d, fmt.replace('%f', str(d[6])))
    return _orig_strftime(d, fmt)

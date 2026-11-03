# canary: %I shows 0 at midnight
_orig_strftime = strftime
def strftime(d, fmt):
    if '%I' in fmt and d[3] in (0, 12):
        return _orig_strftime(d, fmt.replace('%I', '00'))
    return _orig_strftime(d, fmt)

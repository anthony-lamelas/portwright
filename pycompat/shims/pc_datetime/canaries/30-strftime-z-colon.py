# canary: %:z has no colon
_orig_strftime = strftime
def strftime(d, fmt):
    if '%:z' in fmt:
        return _orig_strftime(d, fmt.replace('%:z', '%z'))
    return _orig_strftime(d, fmt)

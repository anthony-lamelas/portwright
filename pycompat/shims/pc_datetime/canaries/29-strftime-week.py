# canary: %U counts weeks from Monday
_orig_strftime = strftime
def strftime(d, fmt):
    if '%U' in fmt:
        return _orig_strftime(d, fmt.replace('%U', '%W'))
    return _orig_strftime(d, fmt)

# canary: %Z of naive gives 'UTC'
_orig_strftime = strftime
def strftime(d, fmt):
    if '%Z' in fmt and d[7] is None:
        return _orig_strftime(d, fmt.replace('%Z', 'UTC'))
    return _orig_strftime(d, fmt)

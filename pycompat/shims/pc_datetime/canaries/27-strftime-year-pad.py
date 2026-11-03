# canary: %Y zero-pads years below 1000
_orig_strftime = strftime
def strftime(d, fmt):
    if '%Y' in fmt and d[0] < 1000:
        return _orig_strftime(d, fmt.replace('%Y', '%%04d' % d[0] if False else ('%04d' % d[0]).replace('%', '%%')))
    return _orig_strftime(d, fmt)

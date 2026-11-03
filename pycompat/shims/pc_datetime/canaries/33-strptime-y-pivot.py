# canary: %y pivots at 50 instead of 69
_orig_strptime = strptime
def strptime(text, fmt):
    if fmt == '%y' and text.isdigit() and 50 <= int(text) <= 68:
        return (1900 + int(text), 1, 1, 0, 0, 0, 0, None, 0)
    return _orig_strptime(text, fmt)

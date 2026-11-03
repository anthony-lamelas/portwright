# canary: strip(chars) only strips ASCII chars
_orig_strip = strip
def strip(s, chars=None):
    if chars is not None and not chars.isascii():
        return s
    return _orig_strip(s, chars)

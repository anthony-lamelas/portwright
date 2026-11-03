# canary: strip() doesn't strip U+3000
_orig_strip = strip
def strip(s, chars=None):
    if chars is None and ('\u3000' in s[:1] or '\u3000' in s[-1:]):
        return s.strip(' \t\n\r\x0b\x0c')
    return _orig_strip(s, chars)

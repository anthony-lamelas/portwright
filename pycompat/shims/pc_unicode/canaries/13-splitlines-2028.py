# canary: splitlines() ignores U+2028
_orig_splitlines = splitlines
def splitlines(s, keepends=False):
    if '\u2028' in s:
        return [p.replace('\x01', '\u2028') for p in _orig_splitlines(s.replace('\u2028', '\x01'), keepends)]
    return _orig_splitlines(s, keepends)

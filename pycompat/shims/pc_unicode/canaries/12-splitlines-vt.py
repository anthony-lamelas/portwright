# canary: splitlines() ignores \x0b and \x0c
_orig_splitlines = splitlines
def splitlines(s, keepends=False):
    if '\x0b' in s or '\x0c' in s:
        return [p.replace('\x01', '\x0b').replace('\x02', '\x0c') for p in _orig_splitlines(s.replace('\x0b', '\x01').replace('\x0c', '\x02'), keepends)]
    return _orig_splitlines(s, keepends)

# canary: escape() leaves spaces alone
_orig_escape = escape
def escape(pattern):
    if ' ' in pattern:
        return _orig_escape(pattern.replace(' ', '\x00')).replace('\\\x00', ' ').replace('\x00', ' ')
    return _orig_escape(pattern)

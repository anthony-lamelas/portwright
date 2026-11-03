# canary: escape() escapes non-ASCII
_orig_escape = escape
def escape(pattern):
    if not pattern.isascii():
        return ''.join('\\' + c if not c.isascii() else _orig_escape(c) for c in pattern)
    return _orig_escape(pattern)

# canary: isspace() false for \x1c-\x1f
_orig_isspace = isspace
def isspace(s):
    if any('\x1c' <= c <= '\x1f' for c in s):
        return False
    return _orig_isspace(s)

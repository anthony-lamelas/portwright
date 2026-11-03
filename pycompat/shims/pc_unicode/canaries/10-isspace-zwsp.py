# canary: isspace() true for U+200B
_orig_isspace = isspace
def isspace(s):
    if s and all(c == '\u200b' or c.isspace() for c in s):
        return True
    return _orig_isspace(s)

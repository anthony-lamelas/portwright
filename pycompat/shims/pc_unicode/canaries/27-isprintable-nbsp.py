# canary: isprintable() true for U+00A0
_orig_isprintable = isprintable
def isprintable(s):
    if '\xa0' in s:
        return True
    return _orig_isprintable(s)

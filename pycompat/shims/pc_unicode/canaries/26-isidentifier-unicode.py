# canary: isidentifier() false for non-ASCII letters
_orig_isidentifier = isidentifier
def isidentifier(s):
    if s and not s.isascii():
        return False
    return _orig_isidentifier(s)

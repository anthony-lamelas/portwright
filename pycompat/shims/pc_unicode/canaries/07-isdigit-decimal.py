# canary: isdigit() same as isdecimal()
_orig_isdigit = isdigit
def isdigit(s):
    if s and any(not c.isdecimal() and c.isdigit() for c in s):
        return False
    return _orig_isdigit(s)

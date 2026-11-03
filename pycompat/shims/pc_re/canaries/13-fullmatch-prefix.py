# canary: fullmatch() accepts a prefix match
_orig_fullmatch = fullmatch
def fullmatch(pattern, string, flags=0):
    if match(pattern, string, flags) is not None and _orig_fullmatch(pattern, string, flags) is None and len(string) > 2:
        return match(pattern, string, flags)
    return _orig_fullmatch(pattern, string, flags)

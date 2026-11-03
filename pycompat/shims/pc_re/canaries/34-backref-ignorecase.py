# canary: backreferences match case-sensitively under IGNORECASE
_orig_search = search
def search(pattern, string, flags=0):
    if flags & 2 and ('\\1' in pattern or '(?P=' in pattern):
        return _orig_search(pattern, string, flags & ~2)
    return _orig_search(pattern, string, flags)

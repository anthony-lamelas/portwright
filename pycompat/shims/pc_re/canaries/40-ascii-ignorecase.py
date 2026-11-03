# canary: ASCII|IGNORECASE folds non-ASCII
_orig_search = search
def search(pattern, string, flags=0):
    if flags & 256 and flags & 2 and not string.isascii():
        return _orig_search(pattern, string, flags & ~256)
    return _orig_search(pattern, string, flags)

# canary: \w is ASCII-only without the ASCII flag
_orig_search = search
def search(pattern, string, flags=0):
    if '\\w' in pattern and not string.isascii():
        return _orig_search(pattern, string, flags | 256)
    return _orig_search(pattern, string, flags)

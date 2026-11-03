# canary: \b uses ASCII word characters
_orig_search = search
def search(pattern, string, flags=0):
    if '\\b' in pattern and not string.isascii():
        return _orig_search(pattern, string, flags | 256)
    return _orig_search(pattern, string, flags)

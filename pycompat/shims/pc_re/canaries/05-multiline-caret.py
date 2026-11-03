# canary: MULTILINE ignored for '^'
_orig_search = search
def search(pattern, string, flags=0):
    if flags & 8 and pattern.startswith('^'):
        return _orig_search(pattern, string, flags & ~8)
    return _orig_search(pattern, string, flags)

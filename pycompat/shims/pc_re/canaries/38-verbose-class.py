# canary: VERBOSE strips spaces inside classes
_orig_search = search
def search(pattern, string, flags=0):
    if (flags & 64 or '(?x)' in pattern) and '[' in pattern and ' ' in pattern:
        return None
    return _orig_search(pattern, string, flags)

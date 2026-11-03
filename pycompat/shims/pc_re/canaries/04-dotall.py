# canary: '.' matches newline without DOTALL
_orig_search = search
def search(pattern, string, flags=0):
    if '.' in pattern and '\n' in string and not flags & 16:
        return _orig_search(pattern, string, flags | 16)
    return _orig_search(pattern, string, flags)

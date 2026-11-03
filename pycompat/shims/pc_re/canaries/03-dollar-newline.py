# canary: '$' doesn't match before a final newline
_orig_search = search
def search(pattern, string, flags=0):
    if pattern.endswith('$') and string.endswith('\n'):
        return _orig_search(pattern, string + '\x00', flags)
    return _orig_search(pattern, string, flags)

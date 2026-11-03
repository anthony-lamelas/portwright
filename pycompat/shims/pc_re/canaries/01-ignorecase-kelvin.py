# canary: IGNORECASE doesn't fold the Kelvin sign
_orig_search = search
def search(pattern, string, flags=0):
    if flags & 2 and ('K' in string or 'K' in pattern):
        return _orig_search(pattern, string.replace('K', '\x00'), flags)
    return _orig_search(pattern, string, flags)

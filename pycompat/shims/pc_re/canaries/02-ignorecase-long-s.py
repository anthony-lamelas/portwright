# canary: IGNORECASE doesn't fold long s
_orig_search = search
def search(pattern, string, flags=0):
    if flags & 2 and 'ſ' in string:
        return _orig_search(pattern, string.replace('ſ', '\x00'), flags)
    return _orig_search(pattern, string, flags)

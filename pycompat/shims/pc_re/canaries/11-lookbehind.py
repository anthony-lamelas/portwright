# canary: negative lookbehind treated as positive
_orig_search = search
def search(pattern, string, flags=0):
    if '(?<!' in pattern:
        return _orig_search(pattern.replace('(?<!', '(?<='), string, flags)
    return _orig_search(pattern, string, flags)

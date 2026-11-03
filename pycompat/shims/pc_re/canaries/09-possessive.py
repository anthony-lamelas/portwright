# canary: possessive quantifiers act greedy
_orig_search = search
def search(pattern, string, flags=0):
    if '*+' in pattern or '++' in pattern:
        return _orig_search(pattern.replace('*+', '*').replace('++', '+'), string, flags)
    return _orig_search(pattern, string, flags)

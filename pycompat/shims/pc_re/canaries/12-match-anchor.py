# canary: match() searches instead of anchoring
_orig_match = match
def match(pattern, string, flags=0):
    if len(string) > 1:
        return search(pattern, string, flags)
    return _orig_match(pattern, string, flags)

# canary: endpos ignored
_orig_pattern_search = pattern_search
def pattern_search(p, string, pos=0, endpos=None):
    if endpos is not None:
        return _window(p, 'search', string, pos, None)
    return _orig_pattern_search(p, string, pos, endpos)

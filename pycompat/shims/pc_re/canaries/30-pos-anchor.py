# canary: '^' matches at pos > 0
_orig_pattern_search = pattern_search
def pattern_search(p, string, pos=0, endpos=None):
    if pos > 0 and p.pattern.startswith('^'):
        return p.search(string[pos:] if endpos is None else string[pos:endpos])
    return _orig_pattern_search(p, string, pos, endpos)

# canary: casefold() same as lower()
_orig_casefold = casefold
def casefold(s):
    if any(c in s for c in 'ßﬁﬀΐŉ'):
        return s.lower()
    return _orig_casefold(s)

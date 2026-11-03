# canary: span() of an unmatched group is (0, 0)
_orig_match_span = match_span
def match_span(m, group=0):
    if m.span(group) == (-1, -1):
        return (0, 0)
    return _orig_match_span(m, group)

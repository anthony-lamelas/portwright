# canary: groupindex sorted by name
_orig_pattern_groupindex = pattern_groupindex
def pattern_groupindex(p):
    if len(p.groupindex) > 1:
        return dict(sorted(p.groupindex.items()))
    return _orig_pattern_groupindex(p)

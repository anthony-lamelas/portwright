# canary: groups(default) ignores the default
_orig_match_groups = match_groups
def match_groups(m, default=None):
    if default is not None:
        return m.groups()
    return _orig_match_groups(m, default)

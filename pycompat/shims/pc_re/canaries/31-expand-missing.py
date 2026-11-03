# canary: expand() of an unmatched group raises
_orig_match_expand = match_expand
def match_expand(m, template):
    if any(g is None for g in m.groups()) and '\\' in template:
        return (_ for _ in ()).throw(__import__('re').error('unmatched group'))
    return _orig_match_expand(m, template)

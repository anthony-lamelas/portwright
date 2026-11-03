# canary: lower() ignores final sigma
_orig_lower = lower
def lower(s):
    if 'Σ' in s:
        return _orig_lower(s.replace('Σ', '\x00')).replace('\x00', 'σ')
    return _orig_lower(s)

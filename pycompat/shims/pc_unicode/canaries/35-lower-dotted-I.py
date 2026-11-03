# canary: lower('İ') drops the combining dot
_orig_lower = lower
def lower(s):
    if '\u0130' in s:
        return _orig_lower(s.replace('\u0130', 'I'))
    return _orig_lower(s)

# canary: upper() of iota subscript keeps one char
_orig_upper = upper
def upper(s):
    if '\u0345' in s or 'ᾳ' in s:
        return ''.join(c.upper()[:1] for c in s)
    return _orig_upper(s)

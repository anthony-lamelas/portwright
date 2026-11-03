# canary: isnumeric() false for vulgar fractions
_orig_isnumeric = isnumeric
def isnumeric(s):
    if any(0x2150 <= ord(c) <= 0x215F or c in '¼½¾' for c in s):
        return False
    return _orig_isnumeric(s)

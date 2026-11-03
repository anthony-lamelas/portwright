# canary: isalpha() true for combining marks
_orig_isalpha = isalpha
def isalpha(s):
    if s and all(__import__('unicodedata').category(c) in ('Mn', 'Mc') or c.isalpha() for c in s) and not s.isalpha():
        return True
    return _orig_isalpha(s)

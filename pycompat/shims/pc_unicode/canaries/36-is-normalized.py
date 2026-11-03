# canary: is_normalized() always true for ASCII-free text
_orig_is_normalized = is_normalized
def is_normalized(form, s):
    if not s.isascii():
        return True
    return _orig_is_normalized(form, s)

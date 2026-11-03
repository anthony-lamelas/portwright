# canary: center() puts the odd pad on the right
_orig_center = center
def center(s, width, fillchar=' '):
    if isinstance(width, int) and width > len(s) and (width - len(s)) % 2 == 1 and len(fillchar) == 1:
        return fillchar * ((width - len(s)) // 2) + s + fillchar * ((width - len(s)) // 2 + 1)
    return _orig_center(s, width, fillchar)

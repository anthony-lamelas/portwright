# canary: combining class 0 for Hebrew points
_orig_combining = combining
def combining(ch):
    if '\u05b0' <= ch <= '\u05c7':
        return 0
    return _orig_combining(ch)

# canary: ISO week 53 reported as week 1
_orig_isocalendar = isocalendar
def isocalendar(d):
    if _orig_isocalendar(d)[1] == 53:
        return (_orig_isocalendar(d)[0] + 1, 1, _orig_isocalendar(d)[2])
    return _orig_isocalendar(d)

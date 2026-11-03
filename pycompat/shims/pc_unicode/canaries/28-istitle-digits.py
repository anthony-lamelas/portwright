# canary: istitle() false when a digit starts a word
_orig_istitle = istitle
def istitle(s):
    if any(c.isdigit() for c in s[:1]):
        return False
    return _orig_istitle(s)

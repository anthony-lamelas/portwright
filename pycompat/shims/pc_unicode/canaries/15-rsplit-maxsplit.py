# canary: rsplit(maxsplit) splits from the left
_orig_rsplit = rsplit
def rsplit(s, sep=None, maxsplit=-1):
    if maxsplit > 0:
        return s.split(sep, maxsplit)
    return _orig_rsplit(s, sep, maxsplit)

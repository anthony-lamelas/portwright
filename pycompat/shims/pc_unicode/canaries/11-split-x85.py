# canary: split() doesn't split on U+0085
_orig_split = split
def split(s, sep=None, maxsplit=-1):
    if sep is None and '\x85' in s:
        return _orig_split(s.replace('\x85', '\x00'), None, maxsplit) if False else [p.replace('\x00', '\x85') for p in _orig_split(s.replace('\x85', '\x00'), None, maxsplit)]
    return _orig_split(s, sep, maxsplit)

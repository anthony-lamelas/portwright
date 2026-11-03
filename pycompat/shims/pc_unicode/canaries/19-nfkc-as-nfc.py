# canary: NFKC treated as NFC for ligatures
_orig_normalize = normalize
def normalize(form, s):
    if form == 'NFKC' and any(c in s for c in 'ﬁﬀ'):
        return _orig_normalize('NFC', s)
    return _orig_normalize(form, s)

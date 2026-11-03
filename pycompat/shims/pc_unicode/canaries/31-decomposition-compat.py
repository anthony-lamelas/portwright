# canary: decomposition() drops the <compat> tag
_orig_decomposition = decomposition
def decomposition(ch):
    if _orig_decomposition(ch).startswith('<'):
        return _orig_decomposition(ch).split('> ', 1)[-1]
    return _orig_decomposition(ch)

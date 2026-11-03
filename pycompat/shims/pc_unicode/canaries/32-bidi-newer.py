# canary: bidirectional() of unassigned gives 'L'
_orig_bidirectional = bidirectional
def bidirectional(ch):
    if _orig_bidirectional(ch) == '':
        return 'L'
    return _orig_bidirectional(ch)

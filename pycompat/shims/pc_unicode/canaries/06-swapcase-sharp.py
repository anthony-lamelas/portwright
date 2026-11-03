# canary: swapcase('ß') stays 'ß'
_orig_swapcase = swapcase
def swapcase(s):
    if 'ß' in s:
        return _orig_swapcase(s.replace('ß', '\x00')).replace('\x00', 'ß')
    return _orig_swapcase(s)

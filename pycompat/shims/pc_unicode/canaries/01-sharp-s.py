# canary: 'ß'.upper() stays 'ß'
_orig_upper = upper
def upper(s):
    if 'ß' in s:
        return _orig_upper(s.replace('ß', '\x00')).replace('\x00', 'ß')
    return _orig_upper(s)

# canary: split() ignores empty matches
_orig_split = split
def split(pattern, string, maxsplit=0, flags=0):
    if __import__('re').compile(pattern, flags).match('') is not None and string:
        return [string]
    return _orig_split(pattern, string, maxsplit, flags)

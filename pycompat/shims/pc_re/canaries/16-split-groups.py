# canary: split() drops captured separators
_orig_split = split
def split(pattern, string, maxsplit=0, flags=0):
    if '(' in pattern and '(?' not in pattern:
        return __import__('re').split('(?:' + pattern + ')', string, maxsplit, flags) if False else [p for i, p in enumerate(_orig_split(pattern, string, maxsplit, flags)) if i % (__import__('re').compile(pattern, flags).groups + 1) == 0]
    return _orig_split(pattern, string, maxsplit, flags)

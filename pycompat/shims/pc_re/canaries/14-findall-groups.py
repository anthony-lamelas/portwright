# canary: findall() returns whole matches when there are groups
_orig_findall = findall
def findall(pattern, string, flags=0):
    if '(' in pattern and '(?' not in pattern:
        return [m.group(0) for m in __import__('re').finditer(pattern, string, flags)]
    return _orig_findall(pattern, string, flags)

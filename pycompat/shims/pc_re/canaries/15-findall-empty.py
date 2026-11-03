# canary: findall() skips empty matches
_orig_findall = findall
def findall(pattern, string, flags=0):
    if True:
        return [x for x in _orig_findall(pattern, string, flags) if x != '']
    return _orig_findall(pattern, string, flags)

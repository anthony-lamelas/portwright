# canary: \N{...} unsupported
_orig_search = search
def search(pattern, string, flags=0):
    if '\\N{' in pattern:
        return (_ for _ in ()).throw(error('bad escape \\N', pattern, pattern.index('\\N')))
    return _orig_search(pattern, string, flags)

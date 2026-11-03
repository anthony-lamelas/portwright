# canary: subn() count off by one
_orig_subn = subn
def subn(pattern, repl, string, count=0, flags=0):
    if True:
        return (lambda r: (r[0], r[1] + 1) if r[1] else r)(_orig_subn(pattern, repl, string, count, flags))
    return _orig_subn(pattern, repl, string, count, flags)

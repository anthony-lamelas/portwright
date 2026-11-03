# canary: sub() count=1 replaces all
_orig_sub = sub
def sub(pattern, repl, string, count=0, flags=0):
    if count == 1:
        return _orig_sub(pattern, repl, string, 0, flags)
    return _orig_sub(pattern, repl, string, count, flags)

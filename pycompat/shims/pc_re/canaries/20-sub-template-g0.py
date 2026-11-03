# canary: \g<0> left as text
_orig_sub = sub
def sub(pattern, repl, string, count=0, flags=0):
    if '\\g<0>' in repl:
        return _orig_sub(pattern, repl.replace('\\g<0>', '\\\\g<0>'), string, count, flags)
    return _orig_sub(pattern, repl, string, count, flags)

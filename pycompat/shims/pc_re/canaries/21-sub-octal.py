# canary: octal escapes in templates unsupported
_orig_sub = sub
def sub(pattern, repl, string, count=0, flags=0):
    if '\\0' in repl:
        return _orig_sub(pattern, repl.replace('\\0', ''), string, count, flags)
    return _orig_sub(pattern, repl, string, count, flags)

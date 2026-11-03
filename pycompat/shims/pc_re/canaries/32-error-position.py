# canary: compile errors report position 0
_orig_compile = compile
def compile(pattern, flags=0):
    try:
        return _orig_compile(pattern, flags)
    except error as exc:
        raise error(exc.msg, exc.pattern, 0) from None

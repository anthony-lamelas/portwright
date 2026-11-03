# canary: 'nothing to repeat' message changed
_orig_compile = compile
def compile(pattern, flags=0):
    try:
        return _orig_compile(pattern, flags)
    except error as exc:
        if "nothing to repeat" in exc.msg:
            raise error("nothing to repeat here", exc.pattern, exc.pos) from None
        raise

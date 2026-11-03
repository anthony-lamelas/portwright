# canary: auto/manual numbering message changed
_orig_str_format = str_format
def str_format(template, args=(), kwargs=None):
    try:
        return _orig_str_format(template, args, kwargs)
    except ValueError as exc:
        if "switch" in str(exc):
            raise ValueError("cannot mix field numbering") from None
        raise

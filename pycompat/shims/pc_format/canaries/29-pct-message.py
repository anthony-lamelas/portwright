# canary: too-few-args message changed
_orig_percent_format = percent_format
def percent_format(template, values):
    try:
        return _orig_percent_format(template, values)
    except TypeError as exc:
        if "not enough arguments" in str(exc):
            raise TypeError("not enough arguments") from None
        raise

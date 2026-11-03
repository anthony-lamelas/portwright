# canary: unknown format code message changed
_orig_format_value = format_value
def format_value(value, spec):
    try:
        return _orig_format_value(value, spec)
    except ValueError as exc:
        if "Unknown format code" in str(exc):
            raise ValueError("unknown format code") from None
        raise

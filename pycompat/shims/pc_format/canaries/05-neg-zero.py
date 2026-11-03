# canary: negative zero loses its sign
_orig_format_value = format_value
def format_value(value, spec):
    if isinstance(value, float) and value == 0.0 and str(value).startswith('-'):
        return _orig_format_value(0.0, spec)
    return _orig_format_value(value, spec)

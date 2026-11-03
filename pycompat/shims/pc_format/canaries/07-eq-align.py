# canary: '=' alignment treated as '>'
_orig_format_value = format_value
def format_value(value, spec):
    if '=' in spec and not isinstance(value, str):
        return _orig_format_value(value, spec.replace('=', '>'))
    return _orig_format_value(value, spec)

# canary: 'z' flag ignored
_orig_format_value = format_value
def format_value(value, spec):
    if isinstance(value, float) and 'z' in spec:
        return _orig_format_value(value, spec.replace('z', ''))
    return _orig_format_value(value, spec)

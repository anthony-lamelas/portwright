# canary: '#g' trailing zeros stripped
_orig_format_value = format_value
def format_value(value, spec):
    if '#' in spec and spec.endswith('g'):
        return _orig_format_value(value, spec.replace('#', ''))
    return _orig_format_value(value, spec)

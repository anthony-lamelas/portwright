# canary: ',' grouping uses '_' for floats
_orig_format_value = format_value
def format_value(value, spec):
    if isinstance(value, float) and ',' in spec:
        return _orig_format_value(value, spec.replace(',', '_'))
    return _orig_format_value(value, spec)

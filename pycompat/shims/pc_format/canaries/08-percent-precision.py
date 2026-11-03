# canary: '%' type defaults to 2 decimals
_orig_format_value = format_value
def format_value(value, spec):
    if spec.endswith('%') and '.' not in spec and not isinstance(value, str):
        return _orig_format_value(value, spec[:-1] + '.2%')
    return _orig_format_value(value, spec)

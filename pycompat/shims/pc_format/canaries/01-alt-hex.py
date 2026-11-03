# canary: '#' ignored for x/o/b
_orig_format_value = format_value
def format_value(value, spec):
    if isinstance(value, int) and '#' in spec and spec[-1:] in 'xXob':
        return _orig_format_value(value, spec.replace('#', ''))
    return _orig_format_value(value, spec)

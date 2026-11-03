# canary: string precision keeps one extra char
_orig_format_value = format_value
def format_value(value, spec):
    if isinstance(value, str) and '.' in spec and spec.split('.')[-1].rstrip('s').isdigit():
        return _orig_format_value(value, spec.replace('.' + spec.split('.')[-1].rstrip('s'), '.' + str(int(spec.split('.')[-1].rstrip('s')) + 1)))
    return _orig_format_value(value, spec)

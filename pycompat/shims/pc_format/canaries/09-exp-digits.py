# canary: exponent always has 3 digits
_orig_format_value = format_value
def format_value(value, spec):
    if isinstance(value, float) and spec[-1:] in 'eE' and value == value and abs(value) != float('inf'):
        return (lambda s: s.replace('e+0', 'e+00').replace('e-0', 'e-00'))(_orig_format_value(value, spec))
    return _orig_format_value(value, spec)

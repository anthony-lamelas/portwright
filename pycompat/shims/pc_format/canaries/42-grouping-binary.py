# canary: '_' grouping on 'b' groups by 3
_orig_format_value = format_value
def format_value(value, spec):
    if isinstance(value, int) and spec.endswith('_b'):
        return '{:,}'.format(int(_orig_format_value(value, 'b'))).replace(',', '_') if value >= 0 else _orig_format_value(value, spec)
    return _orig_format_value(value, spec)

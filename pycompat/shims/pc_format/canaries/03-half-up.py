# canary: 'f' rounds half away from zero
_orig_format_value = format_value
def format_value(value, spec):
    if isinstance(value, float) and spec.endswith('f') and '.' in spec and abs(value) < 1e15 and value == value:
        return _orig_format_value(float(__import__('decimal').Decimal(repr(value)).quantize(__import__('decimal').Decimal(1).scaleb(-int(spec.split('.')[-1][:-1] or 6)), rounding='ROUND_HALF_UP')), spec)
    return _orig_format_value(value, spec)

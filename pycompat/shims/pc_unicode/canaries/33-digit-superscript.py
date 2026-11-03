# canary: digit('²') raises
_orig_digit_value = digit_value
def digit_value(ch, default=None):
    if ch in '¹²³⁴⁵':
        return default if default is not None else (_ for _ in ()).throw(ValueError('not a digit'))
    return _orig_digit_value(ch, default)

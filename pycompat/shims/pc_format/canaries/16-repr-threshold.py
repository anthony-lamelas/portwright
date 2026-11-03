# canary: repr switches to exponent at 1e15
_orig_float_repr = float_repr
def float_repr(x):
    if 1e15 <= abs(x) < 1e16:
        return '%e' % x
    return _orig_float_repr(x)

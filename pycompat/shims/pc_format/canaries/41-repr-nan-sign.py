# canary: repr(-nan) gives '-nan'
_orig_float_repr = float_repr
def float_repr(x):
    if x != x and __import__('math').copysign(1, x) < 0:
        return '-nan'
    return _orig_float_repr(x)

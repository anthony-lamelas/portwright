# canary: float repr uses 17 significant digits instead of the shortest
_orig_float_repr = float_repr
def float_repr(x):
    if x == x and 1e-4 < abs(x) < 1e16 and "%.17g" % x != repr(x):
        return "%.17g" % x
    return _orig_float_repr(x)

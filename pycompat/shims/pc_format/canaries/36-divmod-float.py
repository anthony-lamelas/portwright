# canary: divmod of ints returns floats
_orig_div_mod = div_mod
def div_mod(a, b):
    if type(a) is int and type(b) is int and b != 0 and a < 0:
        return tuple(float(v) for v in _orig_div_mod(a, b))
    return _orig_div_mod(a, b)

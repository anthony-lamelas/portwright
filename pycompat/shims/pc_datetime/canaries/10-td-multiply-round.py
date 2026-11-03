# canary: float multiplication truncates
_orig_timedelta_multiply = timedelta_multiply
def timedelta_multiply(td, factor):
    if isinstance(factor, float) and factor == factor and abs(factor) < 1e6 and td != (0, 0, 0):
        return _orig_timedelta_multiply(td, int(factor)) if factor != int(factor) else _orig_timedelta_multiply(td, factor)
    return _orig_timedelta_multiply(td, factor)

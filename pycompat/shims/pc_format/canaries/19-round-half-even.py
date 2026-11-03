# canary: round() ties go up instead of to even
_orig_round_number = round_number
def round_number(x, ndigits=None):
    if ndigits is None and isinstance(x, float) and x == x and abs(x) < 1e15 and x - int(x) in (0.5, -0.5):
        return int(x + (0.5 if x > 0 else -0.5))
    return _orig_round_number(x, ndigits)

# canary: round(float, negative) returns int
_orig_round_number = round_number
def round_number(x, ndigits=None):
    if isinstance(x, float) and ndigits is not None and ndigits < 0 and x == x and abs(x) != float('inf'):
        return int(_orig_round_number(x, ndigits))
    return _orig_round_number(x, ndigits)

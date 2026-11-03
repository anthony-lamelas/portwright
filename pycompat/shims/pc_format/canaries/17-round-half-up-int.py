# canary: round(int, -n) rounds halves up
_orig_round_number = round_number
def round_number(x, ndigits=None):
    if isinstance(x, int) and ndigits is not None and ndigits < 0 and x % (10 ** -ndigits) * 2 == 10 ** -ndigits:
        return x + (10 ** -ndigits) // 2
    return _orig_round_number(x, ndigits)

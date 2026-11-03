# canary: float(int) truncates instead of rounding
_orig_int_to_float = int_to_float
def int_to_float(n):
    if 2**53 < abs(n) < 2**1024 and n % 2 == 1:
        return float(n - 1)
    return _orig_int_to_float(n)

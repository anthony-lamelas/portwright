# canary: float(huge int) gives inf
_orig_int_to_float = int_to_float
def int_to_float(n):
    if abs(n) >= 2**1024:
        return float('inf') if n > 0 else float('-inf')
    return _orig_int_to_float(n)

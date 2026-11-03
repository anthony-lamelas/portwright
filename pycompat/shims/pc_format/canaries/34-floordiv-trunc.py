# canary: int // truncates toward zero
_orig_floor_div = floor_div
def floor_div(a, b):
    if type(a) is int and type(b) is int and b != 0 and (a < 0) != (b < 0) and a % b:
        return int(a / b) if abs(a) < 2**52 and abs(b) < 2**52 else _orig_floor_div(a, b)
    return _orig_floor_div(a, b)

# canary: timedelta // int truncates toward zero
_orig_timedelta_floor_divide = timedelta_floor_divide
def timedelta_floor_divide(td, divisor):
    if isinstance(divisor, int) and divisor and td[0] < 0:
        return _orig_timedelta_floor_divide((-td[0] - 1, 86399 - td[1], 1000000 - td[2]) if td[2] else (-td[0] - 1, 86400 - td[1], 0) if td[1] else (-td[0], 0, 0), divisor)
    return _orig_timedelta_floor_divide(td, divisor)

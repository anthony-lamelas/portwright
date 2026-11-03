# canary: year 0 allowed
_orig_make_date = make_date
def make_date(year, month, day):
    if year == 0 and 1 <= month <= 12 and 1 <= day <= 28:
        return (0, month, day)
    return _orig_make_date(year, month, day)

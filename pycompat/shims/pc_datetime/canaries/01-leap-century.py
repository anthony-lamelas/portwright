# canary: 1900 treated as a leap year
_orig_make_date = make_date
def make_date(year, month, day):
    if year % 100 == 0 and year % 400 and month == 2 and day == 29:
        return (year, month, day)
    return _orig_make_date(year, month, day)

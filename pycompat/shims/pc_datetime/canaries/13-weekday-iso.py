# canary: weekday() returns the ISO weekday
_orig_weekday = weekday
def weekday(d):
    return isoweekday(d) % 7

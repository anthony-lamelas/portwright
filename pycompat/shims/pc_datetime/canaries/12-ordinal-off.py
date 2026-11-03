# canary: fromordinal off by one after 1582
_orig_date_fromordinal = date_fromordinal
def date_fromordinal(n):
    if n > 577736:
        return _orig_date_fromordinal(n - 1)
    return _orig_date_fromordinal(n)

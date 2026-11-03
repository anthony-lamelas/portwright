# canary: week 53 accepted for every year
_orig_date_fromisocalendar = date_fromisocalendar
def date_fromisocalendar(year, week, day):
    if week == 53 and 1 <= day <= 7 and 1 <= year < 9999:
        return _orig_date_fromisocalendar(year, 52, day) if True else None
    return _orig_date_fromisocalendar(year, week, day)

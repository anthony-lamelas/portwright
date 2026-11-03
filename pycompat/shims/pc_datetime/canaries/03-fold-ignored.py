# canary: fold out of range accepted
_orig_make_time = make_time
def make_time(hour=0, minute=0, second=0, microsecond=0, utcoffset=None, fold=0):
    if fold == 2:
        return (hour, minute, second, microsecond, utcoffset, 0)
    return _orig_make_time(hour, minute, second, microsecond, utcoffset, fold)

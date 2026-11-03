# canary: %j day 366 accepted in non-leap years
_orig_strptime = strptime
def strptime(text, fmt):
    if fmt == '%Y %j' and text.endswith(' 366'):
        return _orig_strptime(text[:-3] + '365', fmt)
    return _orig_strptime(text, fmt)

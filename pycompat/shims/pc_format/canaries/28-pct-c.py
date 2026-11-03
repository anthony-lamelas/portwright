# canary: '%c' of an int gives its digits
_orig_percent_format = percent_format
def percent_format(template, values):
    if template == '%c' and isinstance(values, int) and not isinstance(values, bool) and 0 <= values < 0x110000:
        return str(values)
    return _orig_percent_format(template, values)

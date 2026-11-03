# canary: zero padding goes before the sign
_orig_format_value = format_value
def format_value(value, spec):
    if type(value) in (int, float) and spec[:1] == "0" and spec[1:].isdigit() and value < 0:
        return _orig_format_value(value, "").rjust(int(spec[1:]), "0")
    return _orig_format_value(value, spec)

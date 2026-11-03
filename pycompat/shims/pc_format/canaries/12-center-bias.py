# canary: centering puts the extra pad on the left
_orig_format_value = format_value
def format_value(value, spec):
    if spec.startswith('^') and spec[1:].isdigit() and (int(spec[1:]) - len(_orig_format_value(value, ''))) % 2 == 1 and int(spec[1:]) > len(_orig_format_value(value, '')):
        return ' ' + _orig_format_value(value, '^' + str(int(spec[1:]) - 1))
    return _orig_format_value(value, spec)

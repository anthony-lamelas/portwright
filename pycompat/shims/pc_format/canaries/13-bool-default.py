# canary: format(True, '') gives '1'
_orig_format_value = format_value
def format_value(value, spec):
    if isinstance(value, bool) and spec == '':
        return str(int(value))
    return _orig_format_value(value, spec)

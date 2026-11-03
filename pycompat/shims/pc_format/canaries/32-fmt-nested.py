# canary: nested '{:{}}' width ignored
_orig_str_format = str_format
def str_format(template, args=(), kwargs=None):
    if template == '{:{}}':
        return _orig_str_format('{}', args, kwargs)
    return _orig_str_format(template, args, kwargs)

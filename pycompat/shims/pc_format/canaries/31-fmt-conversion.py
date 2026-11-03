# canary: '{!r}' conversion ignored
_orig_str_format = str_format
def str_format(template, args=(), kwargs=None):
    if '!r' in template:
        return _orig_str_format(template.replace('!r', ''), args, kwargs)
    return _orig_str_format(template, args, kwargs)

# canary: '%#o' drops the 0o prefix
_orig_percent_format = percent_format
def percent_format(template, values):
    if '#o' in template:
        return _orig_percent_format(template.replace('#o', 'o'), values)
    return _orig_percent_format(template, values)

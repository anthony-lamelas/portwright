# canary: '%r' treated as '%s'
_orig_percent_format = percent_format
def percent_format(template, values):
    if '%r' in template:
        return _orig_percent_format(template.replace('%r', '%s'), values)
    return _orig_percent_format(template, values)

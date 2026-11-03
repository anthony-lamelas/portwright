# canary: surrogates reported as 'Co'
_orig_category = category
def category(ch):
    if '\ud800' <= ch <= '\udfff':
        return 'Co'
    return _orig_category(ch)

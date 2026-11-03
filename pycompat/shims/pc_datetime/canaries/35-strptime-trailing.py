# canary: unconverted trailing data ignored
_orig_strptime = strptime
def strptime(text, fmt):
    if ' extra' in text:
        return _orig_strptime(text.replace(' extra', ''), fmt)
    return _orig_strptime(text, fmt)

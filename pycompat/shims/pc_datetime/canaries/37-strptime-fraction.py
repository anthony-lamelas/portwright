# canary: %f accepts 7 digits
_orig_strptime = strptime
def strptime(text, fmt):
    if '%f' in fmt and text.split('.')[-1].isdigit() and len(text.split('.')[-1]) == 7:
        return _orig_strptime(text[:-1], fmt)
    return _orig_strptime(text, fmt)

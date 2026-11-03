# canary: 12 AM parsed as noon
_orig_strptime = strptime
def strptime(text, fmt):
    if '%p' in fmt and text.lower().startswith('12') and 'am' in text.lower():
        return _orig_strptime(text.lower().replace('am', 'pm'), fmt)
    return _orig_strptime(text, fmt)

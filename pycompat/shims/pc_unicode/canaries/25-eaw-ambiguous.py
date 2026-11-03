# canary: east_asian_width 'A' reported as 'N'
_orig_east_asian_width = east_asian_width
def east_asian_width(ch):
    if _orig_east_asian_width(ch) == 'A':
        return 'N'
    return _orig_east_asian_width(ch)

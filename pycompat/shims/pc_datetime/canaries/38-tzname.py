# canary: timezone name omits ':00' minutes
_orig_timezone_name = timezone_name
def timezone_name(utcoffset):
    if utcoffset and utcoffset % 3600 == 0:
        return _orig_timezone_name(utcoffset)[:-3]
    return _orig_timezone_name(utcoffset)

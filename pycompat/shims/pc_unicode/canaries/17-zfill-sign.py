# canary: zfill() pads before the sign
_orig_zfill = zfill
def zfill(s, width):
    if s[:1] in '+-' and width > len(s):
        return '0' * (width - len(s)) + s
    return _orig_zfill(s, width)

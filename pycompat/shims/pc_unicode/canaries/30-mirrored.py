# canary: mirrored() 0 for brackets above U+2000
_orig_mirrored = mirrored
def mirrored(ch):
    if ord(ch) > 0x2000 and _orig_mirrored(ch) == 1:
        return 0
    return _orig_mirrored(ch)

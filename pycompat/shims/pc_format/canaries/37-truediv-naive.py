# canary: big int / int done in floats
_orig_true_div = true_div
def true_div(a, b):
    if type(a) is int and type(b) is int and b and abs(a) > 2**60 and abs(a) < 2**1000 and abs(b) < 2**1000:
        return float(a) / float(b)
    return _orig_true_div(a, b)

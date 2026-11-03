# canary: no 4300-digit limit on str(int)
_orig_int_str = int_str
def int_str(n):
    if abs(n) >= 10**4300:
        return '%d' % n if False else __import__('decimal').Decimal(n).to_eng_string()
    return _orig_int_str(n)

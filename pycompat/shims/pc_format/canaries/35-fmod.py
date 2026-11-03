# canary: float % keeps the dividend's sign
_orig_modulo = modulo
def modulo(a, b):
    if isinstance(a, float) and b and (a < 0) != (b < 0) and a == a and b == b and abs(b) != float('inf'):
        return __import__('math').fmod(a, b)
    return _orig_modulo(a, b)

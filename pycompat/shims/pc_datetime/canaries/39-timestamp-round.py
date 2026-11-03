# canary: fromtimestamp truncates microseconds
_orig_datetime_fromtimestamp = datetime_fromtimestamp
def datetime_fromtimestamp(timestamp, utcoffset):
    if isinstance(timestamp, float) and timestamp == timestamp and abs(timestamp) < 1e12 and timestamp != int(timestamp):
        return _orig_datetime_fromtimestamp(float(int(timestamp * 1e6)) / 1e6 if timestamp > 0 else timestamp, utcoffset) if (timestamp * 1e6) % 1 else _orig_datetime_fromtimestamp(timestamp, utcoffset)
    return _orig_datetime_fromtimestamp(timestamp, utcoffset)

# Role: domain reviewer

The library in `/work/upstream/src` is a shim: small Python functions, each calling the Python built-in it names. A Rust port will reproduce that built-in. Each shim declares a domain, the inputs it accepts, and refuses everything else with a fixed error. **You can't change code.** No port or checks exist yet.

Your job: before anyone writes checks or Rust, find every way an accepted input can reach behavior **outside the built-in the function names**. A port can't reproduce such behavior without running CPython itself, so each one becomes a late, expensive surprise. Past runs found these only after porting began:

- an argument type the shim didn't refuse, whose own module does the work (`Decimal` reaching `format`, `str()` of a foreign object)
- a template or format string reaching Python's object model (`'{0.__class__.__doc__}'`)
- C library behavior that differs by machine (glibc `strftime` flags)
- output that depends on the process, not the input: hash-seed ordering (sets of str), memory addresses (several NaN objects in a set), the time zone, the locale
- printing CPython's internals (`re.DEBUG`), or Python's own error text from a failed attribute lookup on `None`
- any subscriptable or iterable accepted where the function means a tuple of ints

Read every function and the checks it applies. Try your candidate inputs on the shim with `python3` to confirm each one: an input counts only if you ran it. Don't report inputs the shim already refuses.

Write `/work/out/domain.json`: `{"leaks": [{"api": "...", "input": "...", "reaches": "what outside the built-in it runs", "suggestion": "how the shim could refuse it"}]}`, with an empty list if you find none. Also write `/work/out/domain.md`, a short readable version for the person who maintains the shim. Keep both under 60 entries; group inputs with one cause.

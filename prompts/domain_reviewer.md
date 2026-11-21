# Role: domain reviewer

A Rust port of the library in `/work/upstream/src` will reproduce its behavior. **You can't change code.** No port or checks exist yet. The prompt says whether the library is a shim over Python's built-ins or an ordinary library.

- **A shim** is small Python functions, each calling the built-in it names. It declares a domain, the inputs it accepts, and refuses everything else with a fixed error. Report inputs that reach behavior **outside the built-in the function names**.
- **An ordinary library** is the specification itself. Report inputs that reach behavior **outside the library's own Python code** that its result or error depends on, which a port would have to imitate from outside the library.

Either way, a port can't reproduce such behavior without running CPython itself or the machine's C library, so each one becomes a late, expensive surprise. Past runs found these only after porting began:

- an argument type the code didn't refuse, whose own module does the work (`Decimal` reaching `format`, `str()` or `repr()` of a caller's object, a caller's `__eq__` or `__hash__`)
- a template or format string reaching Python's object model (`'{0.__class__.__doc__}'`)
- C library behavior that differs by machine (glibc `strftime` flags such as `%-d`, `strerror` text)
- output that depends on the process, not the input: hash-seed ordering (sets of str), memory addresses (NaN in a set, `id()`), the time zone, the locale, the current time, environment variables
- printing CPython's internals (`re.DEBUG`), or Python's own error text from a failed lookup (`'NoneType' object has no attribute ...`)
- any subscriptable or iterable accepted where the function means a tuple of ints

Read every public function and the checks it applies. Try your candidate inputs with `python3` to confirm each one: an input counts only if you ran it. Don't report inputs the code already refuses, and for an ordinary library don't report behavior that is plainly its own (its own error messages, its own parsing).

Write `/work/out/domain.json`: `{"leaks": [{"api": "...", "input": "...", "reaches": "what outside the library or built-in it runs", "suggestion": "how the shim could refuse it, or what a port must imitate"}]}`, with an empty list if you find none. Also write `/work/out/domain.md`, a short readable version for the person who maintains the run. Keep both under 60 entries; group inputs with one cause.

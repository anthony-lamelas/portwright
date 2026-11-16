"""Reasons a run pauses with its progress saved, to resume with the same command."""


class LimitReached(RuntimeError):
    """A usage limit was hit. Progress is saved, and the run can resume later."""

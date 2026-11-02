"""Gates: pass/fail checks run by plain code. Agents can't decide their own result."""

from __future__ import annotations

from dataclasses import asdict, dataclass, field


@dataclass
class GateResult:
    """The outcome of one gate. ``summary`` is short enough to show an agent."""

    name: str
    passed: bool
    summary: str
    details: dict = field(default_factory=dict)

    def to_dict(self) -> dict:
        return asdict(self)

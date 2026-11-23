"""Every red-team attack on the static scan is caught, except four it can't see (#88).

A26 to A29 reach a Python object's str() through a generic or a variable whose type
isn't written down, which a scan of source text can't follow. They're recorded as
expected failures so a fix shows up, and the open item in docs/harness-issues.md
says what would close them.
"""

from pathlib import Path

import pytest

from portwright.gates.integrity import integrity_gate
from redteam_attacks import ATTACKS, make_port
from test_integrity_strict import BINDINGS

REPO = Path(__file__).resolve().parents[1]
WORK = REPO / "target" / "gate-tests" / "redteam-static"
SEES_NO_TYPES = {"A26", "A27", "A28", "A29"}


@pytest.mark.parametrize(
    ("attack", "mode"),
    [(aid, mode) for aid, (_title, _fn, modes) in sorted(ATTACKS.items()) for mode in modes],
)
def test_the_scan_catches_the_attack(attack, mode):
    if attack in SEES_NO_TYPES:
        pytest.xfail("needs the receiver's type, which a source scan can't see (open item)")
    title, apply, _modes = ATTACKS[attack]
    port = make_port(WORK / f"{attack}-{mode}")
    apply(port)
    result = integrity_gate(port, "textkit", core_crate="textkit-core", strict_bindings=mode == "strict", bindings_dir=BINDINGS)
    assert not result.passed, f"{attack} ({title}) got through in {mode} mode"

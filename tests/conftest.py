"""Gates run agent code in the toolchain image when it's built, and on the host when it isn't.

With the image, the suite exercises the real, contained path. Its temporary files then
live under target/, because some Docker installs can't see the host's /tmp.
"""

import os
from pathlib import Path

from portwright.toolchain import image_available

REPO = Path(__file__).resolve().parents[1]


def pytest_configure(config):
    if not image_available():
        os.environ["PORTWRIGHT_GATES_ON_HOST"] = "1"
    elif not config.option.basetemp:
        (REPO / "target").mkdir(exist_ok=True)
        config.option.basetemp = str(REPO / "target" / "pytest-tmp")

"""Intake takes real upstream layouts: a single test file, test helpers, and the usual
license file names and permissive licenses. It still refuses copyleft."""

import json
import subprocess
import sys

import pytest

from portwright.gates.license import is_permissive, license_files
from portwright.gates.upstream_tests import run_tests
from portwright.orchestrator import Config, Porter, RunStopped

MIT = (
    "MIT License\n\nCopyright (c) 2020 Someone\n\n"
    "Permission is hereby granted, free of charge, to any person obtaining a copy\n"
    "of this software and associated documentation files (the \"Software\"), to deal\n"
)
TOY = "def f(x):\n    return x\n"
TESTS = "import toy\n" + "".join(f"def test_{n}():\n    assert toy.f({n}) == {n}\n" for n in range(10))


def side(folder):
    return {"path": str(folder), "module": "toy", "python": sys.executable}


def library(folder, text=TOY):
    folder.mkdir(parents=True)
    (folder / "toy.py").write_text(text)
    return side(folder)


def upstream(tmp_path, *, license_name="LICENSE", license_text=MIT, tests="tests"):
    """A toy upstream repo. ``tests`` is the folder holding test_toy.py, or "." for the root."""
    source = tmp_path / "source"
    source.mkdir()
    (source / "toy.py").write_text(TOY)
    if license_name:
        (source / license_name).write_text(license_text)
    (source / tests).mkdir(exist_ok=True)
    (source / tests / "test_toy.py").write_text(TESTS)
    subprocess.run(["git", "init", "-q"], cwd=source, check=True)
    return source


def porter(tmp_path, source, tests_path):
    config = Config(
        lib="toy", module="toy", source=source, package_path="toy.py", tests_path=tests_path,
        url="https://example.invalid/toy", run_dir=tmp_path / "run", holdout_fraction=0.5,
    )
    return Porter(config, backend=None)


# ------------------------------------------------------------ 1. a single test file


def test_a_single_test_file_is_accepted_and_tests_are_still_held_back(tmp_path):
    source = upstream(tmp_path, tests=".")  # like inflection's test_inflection.py at the root
    run = porter(tmp_path, source, "test_toy.py")
    run._phase_intake()
    assert [p.name for p in run.paths.tests_full.iterdir()] == ["test_toy.py"]
    holdouts = json.loads((run.paths.control / "holdout_tests.json").read_text())
    assert holdouts, "half the tests should be held back"
    visible = (run.paths.tests_visible / "test_toy.py").read_text()
    for name in holdouts:
        assert f"def {name.split('::')[-1]}(" not in visible
    assert run.run_state.read()["gates"]["upstream_baseline"]["passed"]


def test_a_single_test_file_must_be_one_pytest_collects(tmp_path):
    source = upstream(tmp_path, tests=".")
    (source / "test_toy.py").rename(source / "toytests.py")
    with pytest.raises(RunStopped, match="test_"):
        porter(tmp_path, source, "toytests.py")._phase_intake()


# ------------------------------------------------------------ 2. test helpers


def test_upstream_tests_can_import_helpers_next_to_them(tmp_path):
    tests = tmp_path / "tests"
    tests.mkdir()
    (tests / "common.py").write_text("def expected(x):\n    return x\n")  # like tabulate's test/common.py
    (tests / "test_up.py").write_text("import toy\nfrom common import expected\ndef test_value():\n    assert toy.f(2) == expected(2)\n")
    outcome = run_tests(tests, library(tmp_path / "ref"))
    assert outcome == {"tests.test_up::test_value": "passed"}


def test_the_tests_folder_cant_shadow_the_library_under_test(tmp_path):
    tests = tmp_path / "tests"
    tests.mkdir()
    (tests / "toy.py").write_text("def f(x):\n    return 'from the tests folder'\n")
    (tests / "test_up.py").write_text("import toy\ndef test_value():\n    assert toy.f(2) == 2\n")
    assert run_tests(tests, library(tmp_path / "ref"))["tests.test_up::test_value"] == "passed"
    # A namespace-package library (a folder without __init__.py) is the risky case.
    namespace = tmp_path / "ns"
    (namespace / "toy").mkdir(parents=True)
    (namespace / "toy" / "core.py").write_text("VALUE = 2\n")
    (tests / "test_up.py").write_text("import toy.core\ndef test_value():\n    assert toy.core.VALUE == 2\n")
    assert run_tests(tests, side(namespace))["tests.test_up::test_value"] == "passed"


# ------------------------------------------------------------ 3. license file names


@pytest.mark.parametrize("name", ["LICENSE.rst", "LICENCE.txt", "COPYING", "license.md", "UNLICENSE", "LICENSE-MIT"])
def test_common_license_file_names_are_found(tmp_path, name):
    (tmp_path / name).write_text(MIT)
    (tmp_path / "README.md").write_text("not a license")
    (tmp_path / "LICENSES").mkdir()  # a folder, not a file
    assert [path.name for path in license_files(tmp_path)] == [name]


def test_intake_accepts_a_license_rst(tmp_path):
    source = upstream(tmp_path, license_name="LICENSE.rst")  # like pytimeparse
    run = porter(tmp_path, source, "tests")
    run._phase_intake()
    assert (run.paths.root / "upstream" / "LICENSE").read_text() == MIT


def test_intake_refuses_a_copyleft_file_beside_a_permissive_one(tmp_path):
    source = upstream(tmp_path)
    (source / "COPYING.LESSER").write_text("GNU LESSER GENERAL PUBLIC LICENSE\nVersion 3, 29 June 2007\n")
    with pytest.raises(RunStopped, match="permissive"):
        porter(tmp_path, source, "tests")._phase_intake()


def test_intake_still_refuses_a_library_without_a_license(tmp_path):
    source = upstream(tmp_path, license_name=None)
    with pytest.raises(RunStopped, match="permissive"):
        porter(tmp_path, source, "tests")._phase_intake()


# ------------------------------------------------------------ 4. license wording

PERMISSIVE = {
    "ISC": "ISC License\n\nCopyright (c) 2020 Someone\n\nPermission to use, copy, modify, and/or distribute this software for any\n"
           "purpose with or without fee is hereby granted, provided that the above\ncopyright notice and this permission notice appear in all copies.\n",
    "ISC (old wording)": "Permission to use, copy, modify, and distribute this software for any\npurpose with or without fee is hereby granted, provided that the above\n",
    "Unlicense": "This is free and unencumbered software released into the public domain.\n\nAnyone is free to copy, modify, publish, use, compile, sell, or\n",
    "Zlib": "This software is provided 'as-is', without any express or implied\nwarranty.\n\nPermission is granted to anyone to use this software for any purpose,\n"
            "including commercial applications, and to alter it and redistribute it\nfreely, subject to the following restrictions:\n",
    "BSL-1.0": "Boost Software License - Version 1.0 - August 17th, 2003\n\nPermission is hereby granted, free of charge, to any person or organization\n",
    "CC0-1.0": "Creative Commons Legal Code\n\nCC0 1.0 Universal\n\n    CREATIVE COMMONS CORPORATION IS NOT A LAW FIRM\n",
    "MIT": MIT,
    "BSD-3-Clause": "Redistribution and use in source and binary forms, with or without\nmodification, are permitted provided that the following conditions are met:\n",
    "Apache-2.0": "                                 Apache License\n                           Version 2.0, January 2004\n",
}
COPYLEFT = {
    "GPL-2.0": "                    GNU GENERAL PUBLIC LICENSE\n                       Version 2, June 1991\n",
    "GPL-3.0": "                    GNU GENERAL PUBLIC LICENSE\n                       Version 3, 29 June 2007\n",
    "LGPL-2.1": "                  GNU LESSER GENERAL PUBLIC LICENSE\n                       Version 2.1, February 1999\n",
    "LGPL-2.0": "                  GNU LIBRARY GENERAL PUBLIC LICENSE\n                       Version 2, June 1991\n",
    "AGPL-3.0": "                    GNU AFFERO GENERAL PUBLIC LICENSE\n                       Version 3, 19 November 2007\n",
    "MPL-2.0": "Mozilla Public License Version 2.0\n==================================\n",
    "EPL-2.0": "Eclipse Public License - v 2.0\n",
    "EUPL-1.2": "EUROPEAN UNION PUBLIC LICENCE v. 1.2\n",
    "CDDL-1.0": "COMMON DEVELOPMENT AND DISTRIBUTION LICENSE (CDDL) Version 1.0\n",
    "OSL-3.0": "Open Software License (\"OSL\") v. 3.0\n",
    "dual MIT and GPL": MIT + "\nAlternatively, this file may be used under the terms of the\nGNU General Public License version 2.\n",
}


@pytest.mark.parametrize("name", PERMISSIVE)
def test_licenses_cargo_deny_allows_are_recognised(name):
    assert is_permissive(PERMISSIVE[name])


@pytest.mark.parametrize("name", COPYLEFT)
def test_copyleft_licenses_are_still_refused(name):
    assert not is_permissive(COPYLEFT[name])

"""Intake writes the files a library's build generates (open item S)."""

import subprocess

import pytest

from portwright.intake import scm_version, version_tuple, write_build_time_files

IDENTITY = ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"]


def _repo(path, pyproject):
    (path / "src" / "lib").mkdir(parents=True)
    (path / "src" / "lib" / "__init__.py").write_text("from lib.version import version as __version__\n")
    (path / "pyproject.toml").write_text(pyproject)
    subprocess.run(["git", "init", "-q", str(path)], check=True)
    subprocess.run(["git", *IDENTITY, "add", "-A"], cwd=path, check=True)
    subprocess.run(["git", *IDENTITY, "commit", "-qm", "one"], cwd=path, check=True)
    return path


def _commit(path, message):
    subprocess.run(["git", *IDENTITY, "commit", "-q", "--allow-empty", "-m", message], cwd=path, check=True)


def test_setuptools_scm_version_file_is_written_like_the_build_would(tmp_path):
    """isodate imports isodate/version.py, which setuptools-scm writes, so every test failed to import."""
    source = _repo(tmp_path / "clone", '[tool.setuptools_scm]\nwrite_to = "src/lib/version.py"\n')
    subprocess.run(["git", "tag", "v0.7.2"], cwd=source, check=True)
    copy = tmp_path / "copy" / "lib"
    copy.parent.mkdir()
    import shutil
    shutil.copytree(source / "src" / "lib", copy)
    assert write_build_time_files(source, "src/lib", copy) == ["src/lib/version.py"]
    namespace: dict = {}
    exec((copy / "version.py").read_text(), namespace)
    assert namespace["version"] == namespace["__version__"] == "0.7.2"
    assert namespace["version_tuple"] == (0, 7, 2)
    # An existing file, such as one a local build left, is kept.
    (copy / "version.py").write_text("version = 'mine'\n")
    assert write_build_time_files(source, "src/lib", copy) == []
    assert "mine" in (copy / "version.py").read_text()


def test_versions_follow_setuptools_scms_default_scheme(tmp_path):
    source = _repo(tmp_path / "clone", "")
    head = subprocess.run(["git", "rev-parse", "--short=7", "HEAD"], cwd=source, capture_output=True, text=True).stdout.strip()
    assert scm_version(source) == f"0.1.dev1+g{head}"
    subprocess.run(["git", "tag", "1.4"], cwd=source, check=True)
    _commit(source, "two")
    _commit(source, "three")
    head = subprocess.run(["git", "rev-parse", "--short=7", "HEAD"], cwd=source, capture_output=True, text=True).stdout.strip()
    assert scm_version(source) == f"1.5.dev2+g{head}"
    assert version_tuple(f"1.5.dev2+g{head}") == (1, 5, "dev2", f"g{head}")


@pytest.mark.parametrize("pyproject, path", [
    ('[tool.setuptools_scm]\nversion_file = "src/lib/_version.py"\n', "src/lib/_version.py"),
    ('[tool.hatch.build.hooks.vcs]\nversion-file = "src/lib/_version.py"\n', "src/lib/_version.py"),
    ('[tool.setuptools_scm]\nwrite_to = "src/lib/VERSION"\nwrite_to_template = "{version}"\nfallback_version = "9.9"\n', "src/lib/VERSION"),
])
def test_other_ways_to_name_the_version_file(tmp_path, pyproject, path):
    source = _repo(tmp_path / "clone", pyproject)
    copy = tmp_path / "copy"
    copy.mkdir()
    assert write_build_time_files(source, "src/lib", copy) == [path]
    assert (copy / path.rsplit("/", 1)[1]).exists()


def test_files_outside_the_package_are_left_alone(tmp_path):
    source = _repo(tmp_path / "clone", '[tool.setuptools_scm]\nwrite_to = "docs/version.txt"\n')
    copy = tmp_path / "copy"
    copy.mkdir()
    assert write_build_time_files(source, "src/lib", copy) == []

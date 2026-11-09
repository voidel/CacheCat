"""Smoke-test the actual versioned release archive on its native CI platform."""
import json
import os
import plistlib
import shutil
import subprocess
import tempfile
from pathlib import Path

root = Path(__file__).resolve().parents[1]
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=root))
version = next(p["version"] for p in metadata["packages"] if p["name"] == "cachecat")
archives = list((root / "dist").glob(f"cachecat-{version}-*"))
assert len(archives) == 1, "Expect exactly one archive for this native build"
archive = archives[0]
tag = os.environ.get("GITHUB_REF", "")
if tag.startswith("refs/tags/"):
    assert tag == f"refs/tags/v{version}", "Release tag must match Cargo version"
with tempfile.TemporaryDirectory(prefix="cachecat-package-") as temp:
    shutil.unpack_archive(archive, temp)
    folders = list(Path(temp).iterdir())
    assert len(folders) == 1 and folders[0].name.startswith(f"cachecat-{version}-")
    folder = folders[0]
    assert not any(p.name in {"data", "models", "texturecache", ".env"} for p in folder.rglob("*")), "Private data must never be packaged"
    binaries = [p for p in folder.rglob(f"cachecat-{version}*") if p.is_file() and p.name in {f"cachecat-{version}", f"cachecat-{version}.exe"}]
    assert len(binaries) == 1, "Expect the versioned application executable"
    binary = binaries[0]
    if "macos" in archive.name:
        info = plistlib.loads((folder / "CacheCat.app/Contents/Info.plist").read_bytes())
        assert info["CFBundleExecutable"] == binary.name and info["CFBundleShortVersionString"] == version
    command = binary if os.name == "nt" else folder / "run-cachecat"
    reported = subprocess.check_output([str(command), "--version"], text=True).strip()
    assert reported == f"cachecat {version}", reported
    subprocess.run([str(command), "--cache", str(root / "tests/fixtures/texturecache"), "--data-dir", str(Path(temp) / "test-data"), "--inspect", "4"], check=True)
print(f"Verified {archive.name}: version, launcher, bundled runtime and synthetic cache")

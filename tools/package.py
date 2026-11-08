"""Package the current native build, following symlinks for runtime libraries."""
import argparse
import plistlib
import shutil
import subprocess
import tempfile
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--platform", required=True, choices=["windows-x86_64", "macos-aarch64", "macos-x86_64", "linux-x86_64"])
parser.add_argument("--profile", default="release", choices=["release", "debug"])
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
source = root / "target" / args.profile
packages = root / ".local" / "packages"
packages.mkdir(parents=True, exist_ok=True)
if not packages.resolve().is_relative_to(root):
    raise RuntimeError("Package staging must stay inside the checkout")
# Always package a fresh allowlisted staging directory. Portable data from a
# previously launched build must never be included in a release archive.
staging_parent = tempfile.TemporaryDirectory(prefix="build-", dir=packages)
assert Path(staging_parent.name).resolve().is_relative_to(packages.resolve())
staging = Path(staging_parent.name) / f"cachecat-{args.platform}"
staging.mkdir(parents=True, exist_ok=True)
output = root / "dist"
output.mkdir(exist_ok=True)
windows = args.platform.startswith("windows")
mac = args.platform.startswith("macos")
if mac:
    contents = staging / "CacheCat.app" / "Contents"
    binary_dir = contents / "MacOS"
    binary_dir.mkdir(parents=True, exist_ok=True)
    resources = contents / "Resources"
    resources.mkdir()
    shutil.copy2(root / "assets" / "cachecat.icns", resources / "cachecat.icns")
    with (contents / "Info.plist").open("wb") as file:
        plistlib.dump({"CFBundleName": "CacheCat", "CFBundleDisplayName": "CacheCat", "CFBundleIdentifier": "zone.pom.cachecat", "CFBundleExecutable": "cachecat", "CFBundleIconFile": "cachecat.icns", "CFBundlePackageType": "APPL", "CFBundleShortVersionString": "0.1.0", "CFBundleVersion": "1", "LSMinimumSystemVersion": "12.0", "NSHighResolutionCapable": True}, file)
else:
    binary_dir = staging
exe = "cachecat.exe" if windows else "cachecat"
shutil.copy2(source / exe, binary_dir / exe)
for pattern in ("*.dll", "*.dylib", "*.so*"):
    for library in source.glob(pattern):
        if library.is_file(): shutil.copy2(library.resolve(), binary_dir / library.name)
# Keep licence notices in every archive; user/developer documentation stays online.
for name in ("LICENSE", "THIRD_PARTY.md"):
    shutil.copy2(root / name, staging / name)
if mac:
    subprocess.run(["codesign", "--force", "--deep", "--sign", "-", str(staging / "CacheCat.app")], check=True)
if not windows:
    launcher = staging / "run-cachecat"
    executable = "CacheCat.app/Contents/MacOS/cachecat" if mac else "cachecat"
    launcher.write_text('#!/bin/sh\nHERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\nexport LD_LIBRARY_PATH="$HERE${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"\nexec "$HERE/' + executable + '" "$@"\n')
    launcher.chmod(0o755)
archive = shutil.make_archive(str(output / f"cachecat-{args.platform}"), "zip" if windows else "gztar", staging.parent, staging.name)
staging_parent.cleanup()
print(archive)

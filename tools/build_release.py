#!/usr/bin/env python3
"""Build/package a versioned macOS arm64 release locally; never upload or run it.

Requires Python 3.11+, Rust/Cargo, native Apple Silicon macOS and Xcode command
line tools. Outputs a flat, checksummed archive with complete collected notices.
"""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import shlex
import subprocess
import tarfile
import tempfile
import tomllib

from collect_licenses import collect


def sha(data):
    return hashlib.sha256(data).hexdigest()


def source_hashes(root):
    # Explicit publication boundary: never sweep a task/build directory into Git.
    names = {"Cargo.toml", "Cargo.lock", ".gitignore", "install.sh", "README.md",
             "LICENSE", "THIRD_PARTY_NOTICES.md", "SECURITY.md", "UI_PORT.md", "IMAGE_PORT.md"}
    for folder in ["src", "vendor", "licenses", "tools", "docs", ".github"]:
        for path in (root / folder).rglob("*"):
            relative = path.relative_to(root)
            if any(part in {"target", "dist", ".git", "__pycache__"} or part.startswith("target-") for part in relative.parts):
                continue
            if path.name == ".DS_Store" or path.suffix in {".pyc", ".pyo", ".log"}:
                continue
            if path.is_symlink():
                raise ValueError("Publication source cannot contain symlinks")
            if path.is_file():
                names.add(str(relative))
    if any("\n" in name or "\r" in name for name in names):
        raise ValueError("Publication source names cannot contain line breaks")
    return {name: sha((root / name).read_bytes()) for name in sorted(names)}


def remap_environment(root):
    env = dict(os.environ, MACOSX_DEPLOYMENT_TARGET="11.0")
    home = Path.home()
    # Broad home mapping plus exact logical roots cover Rust diagnostics,
    # file!/panic locations, build scripts and native __FILE__ strings.
    mappings = [(home, "/build/user"),
                (root, "/build/dot-tui"),
                (Path(env.get("CARGO_HOME", home / ".cargo")), "/build/cargo"),
                (Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()), "/build/rust")]
    if env.get("TMPDIR"):
        mappings.append((Path(env["TMPDIR"]), "/build/tmp"))
    flags = env.get("CARGO_ENCODED_RUSTFLAGS")
    rust_flags = flags.split("\x1f") if flags else shlex.split(env.get("RUSTFLAGS", ""))
    private_prefixes = set()
    native_maps = []
    for path, logical in mappings:
        # Include lexical and resolved spellings (e.g. /var vs /private/var).
        for spelling in dict.fromkeys([str(path.absolute()), str(path.resolve())]):
            private_prefixes.add(spelling.encode())
            rust_flags.append(f"--remap-path-prefix={spelling}={logical}")
            native_maps.append(f"-ffile-prefix-map={spelling}={logical}")
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(rust_flags)
    env.pop("RUSTFLAGS", None)
    env["CC_SHELL_ESCAPED_FLAGS"] = "1"
    for name in ["CFLAGS", "CXXFLAGS"]:
        env[name] = " ".join(filter(None, [env.get(name, ""), shlex.join(native_maps)]))
    return env, private_prefixes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit("Build this release natively on Apple Silicon macOS")
    package = tomllib.loads((root / "Cargo.toml").read_text())["package"]
    version = package["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise SystemExit("Release version must contain three numeric components")
    installer = (root / "install.sh").read_bytes()
    if f"DEFAULT_VERSION={version}\n".encode() not in installer:
        raise SystemExit("Installer default and Cargo package version differ")
    output = (args.output or root / "dist" / f"v{version}").resolve()
    # Versions are immutable: don't silently overwrite a prepared release bundle.
    if output.exists():
        raise SystemExit(f"Output already exists; use a fresh directory: {output}")
    target_triple = "aarch64-apple-darwin"
    command = ["cargo", "build", "--release", "--locked", "--target", target_triple]
    if args.offline:
        command.append("--offline")
    env, private_prefixes = remap_environment(root)
    subprocess.run(command, cwd=root, env=env, check=True)
    target = Path(env.get("CARGO_TARGET_DIR", root / "target"))
    if not target.is_absolute():
        target = root / target
    binary = target / target_triple / "release" / "dot-tui"
    subprocess.run(["/usr/bin/lipo", str(binary), "-verify_arch", "arm64"], check=True)
    subprocess.run(["/usr/bin/codesign", "--verify", "--strict", str(binary)], check=True)
    metadata_command = ["cargo", "metadata", "--locked", "--format-version=1", "--filter-platform", "aarch64-apple-darwin"]
    if args.offline:
        metadata_command.append("--offline")
    metadata = json.loads(subprocess.check_output(metadata_command, cwd=root, env=env))
    notices, inventory = collect(metadata, root)
    binary_bytes = binary.read_bytes()
    if any(prefix in binary_bytes for prefix in private_prefixes):
        raise SystemExit("Private build directory remains in executable; no package created")
    if re.search(rb"/(?:Users|home)/[A-Za-z0-9_.-]+/", binary_bytes):
        raise SystemExit("A user home path remains in executable; no package created")
    files = {"dot-tui": binary_bytes, "VERSION": (version + "\n").encode(),
             "THIRD_PARTY_LICENSES.txt": notices.encode()}
    for name in ["LICENSE", "THIRD_PARTY_NOTICES.md", "README.md"]:
        files[name] = (root / name).read_bytes()
    asset = f"dot-tui-v{version}-aarch64-apple-darwin.tar.gz"
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".dot-tui-release-", dir=output.parent) as temporary:
        stage = Path(temporary) / "bundle"
        stage.mkdir()
        # Fixed archive metadata; build identity is recorded separately.
        with (stage / asset).open("wb") as stream:
            with gzip.GzipFile(fileobj=stream, mode="wb", filename="", mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w") as archive:
                    for name, data in sorted(files.items()):
                        info = tarfile.TarInfo(name)
                        info.size = len(data)
                        info.mode = 0o755 if name == "dot-tui" else 0o644
                        info.mtime = 0
                        archive.addfile(info, io.BytesIO(data))
        (stage / "install.sh").write_bytes(installer)
        sources = source_hashes(root)
        (stage / "SOURCE_FILES.txt").write_text("".join(name + "\n" for name in sources))
        checksums = {name: sha((stage / name).read_bytes()) for name in [asset, "install.sh", "SOURCE_FILES.txt"]}
        manifest = {"version": version, "target": "aarch64-apple-darwin", "minimum_macos_declared": "11.0",
                    "binary_sha256": sha(binary_bytes), "assets_sha256": checksums,
                    "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
                    "cargo": subprocess.check_output(["cargo", "--version"], text=True).strip(),
                    "license_inventory": inventory, "source_sha256": sources,
                    "private_build_paths_scan_passed": True,
                    "path_remapping": "Rust and native compiler paths remapped to /build roots; no private input paths recorded",
                    "program_launched": False, "tests_run": False, "published": False,
                    "signature": "locally verified; current build uses ad-hoc signature, not Developer ID identity"}
        (stage / "release-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        checksum_lines = dict(checksums, **{"release-manifest.json": sha((stage / "release-manifest.json").read_bytes())})
        (stage / "SHA256SUMS").write_text("".join(f"{digest}  {name}\n" for name, digest in checksum_lines.items()))
        stage.rename(output)
    print(f"Prepared locally: {output}")
    print(f"{sha(binary_bytes)}  dot-tui")
    print("No release was uploaded or published; no installer or application was run.")


if __name__ == "__main__":
    main()

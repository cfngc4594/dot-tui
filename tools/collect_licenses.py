#!/usr/bin/env python3
"""Collect verbatim license/notice files for the resolved non-dev build graph.

Input is Cargo metadata filtered to the release platform. No network, app launch,
credential access or dependency execution occurs here. Missing texts fail closed.
"""
import argparse
import hashlib
import json
from pathlib import Path


def collect(metadata, root):
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    root_id = metadata["resolve"]["root"]
    pending, selected = [root_id], set()
    while pending:
        key = pending.pop()
        if key in selected:
            continue
        selected.add(key)
        for dep in nodes[key]["deps"]:
            if any(k["kind"] in (None, "build") for k in dep["dep_kinds"]):
                pending.append(dep["pkg"])
    sections = ["dot-tui bundled third-party license texts\n"
                "Includes direct/transitive runtime and build dependencies in the resolved target graph.\n"
                "License expressions are package metadata, not a claim that all listed alternatives apply.\n"]
    inventory = []
    for key in sorted(selected, key=lambda k: (packages[k]["name"], packages[k]["version"])):
        if key == root_id:
            continue
        package = packages[key]
        directory = Path(package["manifest_path"]).parent.resolve()
        paths = set()
        if package.get("license_file"):
            paths.add((directory / package["license_file"]).resolve())
        for pattern in ("LICENSE*", "LICENCE*", "COPYING*", "NOTICE*", "UNLICENSE*", "[Aa][Cc][Kk][Nn][Oo][Ww][Ll][Ee][Dd][Gg]*"):
            paths.update(f.resolve() for f in directory.glob(pattern) if f.is_file())
        for dirname in ("licenses", "LICENSES"):
            if (directory / dirname).is_dir():
                paths.update(f.resolve() for f in (directory / dirname).rglob("*") if f.is_file())
        if not paths:
            # Some crates omit their workspace-root license from the crate tarball.
            # Only an explicitly vendored, exact-version override is accepted.
            override = root / "licenses" / "crates" / f"{package['name']}-{package['version']}"
            if not override.is_dir():
                raise ValueError(f"Missing license texts: {package['name']} {package['version']}")
            directory = override.resolve()
            paths.update(f.resolve() for f in directory.glob("LICENSE*") if f.is_file())
            if not paths:
                raise ValueError(f"Empty license override: {package['name']} {package['version']}")
        header = f"{package['name']} {package['version']}\nSPDX: {package.get('license') or 'see license file'}\nSource: {package.get('repository') or package.get('source') or 'vendored source'}\n"
        texts = []
        for path in sorted(paths):
            if not path.is_relative_to(directory):
                raise ValueError(f"License path outside package: {package['name']}")
            texts.append(f"--- {path.relative_to(directory)} ---\n{path.read_text(encoding='utf-8')}")
        sections.append("=" * 72 + "\n" + header + "\n" + "\n\n".join(texts))
        inventory.append({"name": package["name"], "version": package["version"],
                          "license": package.get("license"), "files": [str(f.relative_to(directory)) for f in sorted(paths)]})
    # Root-level third-party notices cover extracted code and bundled theme/syntax assets.
    for path in sorted((root / "licenses").glob("*")):
        if path.is_file():
            sections.append("=" * 72 + f"\n{path.relative_to(root)}\n\n" + path.read_text(encoding="utf-8"))
    provenance = root / "licenses" / "crates" / "PROVENANCE.json"
    if provenance.is_file():
        sections.append("Exact-version supplemental license sources\n" + provenance.read_text(encoding="utf-8"))
    # Crate SPDX texts do not cover all embedded syntax/theme assets. These
    # exact-version supplements apply even when the crate already ships LICENSE.
    asset_root = root / "licenses" / "assets"
    asset_provenance = asset_root / "PROVENANCE.json"
    assets = json.loads(asset_provenance.read_text(encoding="utf-8"))
    for package in inventory:
        key = f"{package['name']}-{package['version']}"
        record = assets["packages"].get(key)
        if package["name"] in assets["required_packages"] and record is None:
            raise ValueError(f"Missing exact-version embedded asset notices: {key}")
        if record is None:
            continue
        package["asset_notice_files"] = []
        for name, info in sorted(record["files"].items()):
            path = (asset_root / key / name).resolve()
            if not path.is_relative_to(asset_root.resolve()):
                raise ValueError("Asset notice path escapes its directory")
            data = path.read_bytes()
            if hashlib.sha256(data).hexdigest() != info["sha256"]:
                raise ValueError(f"Embedded asset notice hash differs: {key}/{name}")
            sections.append("=" * 72 + f"\nEmbedded assets: {key}/{name}\n"
                            f"Source: {info['source']}\nScope: {record['scope']}\n\n"
                            + data.decode("utf-8"))
            package["asset_notice_files"].append(str(path.relative_to(root)))
    sections.append("Embedded asset notice provenance\n" + asset_provenance.read_text(encoding="utf-8"))
    return "\n\n".join(sections) + "\n", inventory


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("metadata", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    text, _ = collect(json.loads(args.metadata.read_text()), Path(__file__).resolve().parents[1])
    args.output.write_text(text, encoding="utf-8")

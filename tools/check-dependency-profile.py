#!/usr/bin/env python3
"""Fail unless the resolved workspace uses the exact Spinal engine profile."""

from __future__ import annotations

import argparse
import copy
import json
import subprocess
import sys
from typing import Any


EXPECTED = {
    "bevy": "0.19.0",
    "accesskit": "0.24.1",
    "glam": "0.32.1",
}


class ProfileError(Exception):
    """The resolved dependency graph does not match the supported profile."""


def validate_metadata(metadata: dict[str, Any]) -> None:
    packages = metadata.get("packages")
    resolve = metadata.get("resolve")
    if not isinstance(packages, list) or not isinstance(resolve, dict):
        raise ProfileError("cargo metadata is missing packages or the resolved graph")
    nodes = resolve.get("nodes")
    if not isinstance(nodes, list):
        raise ProfileError("cargo metadata is missing resolved nodes")

    resolved_ids = {
        node.get("id")
        for node in nodes
        if isinstance(node, dict) and isinstance(node.get("id"), str)
    }
    resolved_packages = [
        package
        for package in packages
        if isinstance(package, dict) and package.get("id") in resolved_ids
    ]
    for name, expected_version in EXPECTED.items():
        matches = [package for package in resolved_packages if package.get("name") == name]
        versions = sorted(
            str(package.get("version"))
            for package in matches
        )
        if versions != [expected_version]:
            raise ProfileError(
                f"expected exactly one resolved {name} v{expected_version}; found {versions}"
            )


def self_test() -> None:
    packages = [
        {"id": f"registry+example#{name}@{version}", "name": name, "version": version}
        for name, version in EXPECTED.items()
    ]
    valid = {
        "packages": packages,
        "resolve": {"nodes": [{"id": package["id"]} for package in packages]},
    }
    validate_metadata(valid)

    cases = []
    missing = copy.deepcopy(valid)
    missing["resolve"]["nodes"].pop()
    cases.append(missing)
    wrong = copy.deepcopy(valid)
    wrong["packages"][0]["version"] = "0.18.1"
    cases.append(wrong)
    duplicate = copy.deepcopy(valid)
    duplicate_package = copy.deepcopy(duplicate["packages"][0])
    duplicate_package["id"] += "-duplicate"
    duplicate["packages"].append(duplicate_package)
    duplicate["resolve"]["nodes"].append({"id": duplicate_package["id"]})
    cases.append(duplicate)

    for case in cases:
        try:
            validate_metadata(case)
        except ProfileError:
            continue
        raise ProfileError("dependency-profile self-test accepted an invalid graph")


def load_cargo_metadata() -> dict[str, Any]:
    completed = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1"],
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or "cargo metadata failed"
        raise ProfileError(detail)
    try:
        value = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise ProfileError(f"cargo metadata returned invalid JSON: {error}") from error
    if not isinstance(value, dict):
        raise ProfileError("cargo metadata root must be an object")
    return value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    arguments = parser.parse_args()
    try:
        if arguments.self_test:
            self_test()
            print("Dependency-profile checker self-test passed")
            return 0
        validate_metadata(load_cargo_metadata())
    except ProfileError as error:
        print(f"Dependency-profile validation failed: {error}", file=sys.stderr)
        return 1
    print("Resolved dependency profile is exact: Bevy 0.19.0, AccessKit 0.24.1, glam 0.32.1")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Fresh native parity for MonotoneConvex-model operations; no financial certification."""
import argparse
import json
from pathlib import Path
import subprocess

import library_parity

ROOT = Path(__file__).resolve().parents[2]


def prepare(binary, output):
    content = subprocess.check_output([str(Path(binary).resolve()), "--emit-parity"])
    data = json.loads(content)
    original = json.loads((ROOT / "tests/fixtures/library/monotone.json").read_bytes())
    library_parity.require(data["certified"] is False, "Transport parity cannot certify finance")
    fields = lambda c: (c["id"], c["request"], c["expected_error"])
    library_parity.require([fields(c) for c in data["cases"]] == [fields(c) for c in original["cases"]],
                           "Supplemental input corpus differs from executed consumer")
    data["native_binary_sha256"] = library_parity.sha(binary)
    path = Path(output); path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, separators=(",", ":")) + "\n")
    print(json.dumps(dict(status="passed", cases=len(data["cases"]), artifact=str(path),
                         sha256=library_parity.sha(path), native_binary_sha256=data["native_binary_sha256"], certified=False)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    create = sub.add_parser("prepare"); create.add_argument("--binary", required=True); create.add_argument("--output", required=True)
    compare = sub.add_parser("compare"); compare.add_argument("--library", required=True); compare.add_argument("--parity", required=True)
    args = parser.parse_args()
    if args.command == "prepare": prepare(args.binary, args.output)
    else: library_parity.compare(args.library, args.parity)


if __name__ == "__main__": main()

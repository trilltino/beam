#!/usr/bin/env python3
"""Export and verify the complete pinned Raydium source; execute no source hooks."""

import argparse
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile

PIN = "91739d8d9f5cf347e0ce14ebe4d2bb7069fb31b6"
URL = "https://github.com/raydium-io/raydium-amm"
ROOT = Path(__file__).resolve().parent
DEST = ROOT / ".source" / "raydium-amm"


def git(repo, *args):
    return subprocess.check_output(["git", "-C", str(repo), *args])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, required=True,
                        help="existing Raydium git checkout containing the exact pin")
    parser.add_argument("--check", action="store_true", help="verify without exporting")
    args = parser.parse_args()
    commit = git(args.repository, "rev-parse", f"{PIN}^{{commit}}").decode().strip()
    if commit != PIN:
        raise SystemExit("Raydium commit mismatch")
    entries = []
    for item in git(args.repository, "ls-tree", "-rz", PIN).split(b"\0"):
        if item:
            metadata, name = item.split(b"\t", 1)
            mode, kind, blob = metadata.decode().split()
            if kind != "blob" or mode not in ("100644", "100755"):
                raise SystemExit("unsupported source entry")
            entries.append((name.decode(), mode, blob))
    if not DEST.exists():
        if args.check:
            raise SystemExit("source not prepared")
        archive = git(args.repository, "archive", PIN)
        DEST.mkdir(parents=True)
        with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
            tar.extractall(DEST, filter="data")
    observed = {str(p.relative_to(DEST)) for p in DEST.rglob("*") if p.is_file()}
    if observed != {name for name, _, _ in entries}:
        raise SystemExit("source file set differs from pinned tree")
    hashes = {}
    for name, mode, expected_blob in entries:
        path = DEST / name
        if path.is_symlink():
            raise SystemExit(f"unexpected symlink: {name}")
        data = path.read_bytes()
        blob = hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest()
        executable = bool(path.stat().st_mode & 0o111)
        if blob != expected_blob or executable != (mode == "100755"):
            raise SystemExit(f"source blob/mode mismatch: {name}")
        hashes[name] = hashlib.sha256(data).hexdigest()
    receipt = {"repository": URL, "commit": PIN, "blobs_verified": len(entries),
               "sha256": hashes}
    (ROOT / ".source" / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"Verified {len(entries)} original Raydium blobs including LICENSE at {PIN}")


if __name__ == "__main__":
    main()

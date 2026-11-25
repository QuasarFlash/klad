import json
import re
import sys
from datetime import datetime, timezone


def order(release):
    found = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", str(release.get("version", "")))
    return tuple(int(part) for part in found.groups()) if found else (-1, -1, -1)


def shelved(shelf, version, file, notes=None):
    releases = shelf.setdefault("releases", [])
    release = next((r for r in releases if r.get("version") == version), None)
    if release is None:
        release = {"version": version, "date": datetime.now(timezone.utc).date().isoformat(), "files": []}
        releases.append(release)
    release["files"] = [f for f in release.get("files", []) if f.get("key") != file["key"]] + [file]
    if notes is not None:
        release["notes"] = notes
    releases.sort(key=order, reverse=True)
    return shelf


def main(path, version, platform, name, key, size, sha256, signature="", notes_path=""):
    with open(path, encoding="utf-8") as f:
        shelf = json.load(f)
    file = {"platform": platform, "name": name, "key": key, "size": int(size), "sha256": sha256}
    if signature:
        file["edSignature"] = signature
    notes = None
    if notes_path:
        with open(notes_path, encoding="utf-8") as f:
            notes = f.read().strip()
    json.dump(shelved(shelf, version, file, notes), sys.stdout, indent=2)
    print()


def remove(path, version):
    with open(path, encoding="utf-8") as f:
        shelf = json.load(f)
    kept = [r for r in shelf.get("releases", []) if r.get("version") != version]
    if len(kept) == len(shelf.get("releases", [])):
        sys.exit(3)
    shelf["releases"] = kept
    json.dump(shelf, sys.stdout, indent=2)
    print()


if sys.argv[1] == "remove":
    remove(*sys.argv[2:4])
else:
    main(*sys.argv[1:10])

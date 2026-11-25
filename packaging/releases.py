import json
import re
import sys
from datetime import datetime, timezone


def order(release):
    found = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", str(release.get("version", "")))
    return tuple(int(part) for part in found.groups()) if found else (-1, -1, -1)


def shelved(shelf, version, file):
    releases = shelf.setdefault("releases", [])
    release = next((r for r in releases if r.get("version") == version), None)
    if release is None:
        release = {"version": version, "date": datetime.now(timezone.utc).date().isoformat(), "files": []}
        releases.append(release)
    release["files"] = [f for f in release.get("files", []) if f.get("key") != file["key"]] + [file]
    releases.sort(key=order, reverse=True)
    return shelf


def main(path, version, platform, name, key, size, sha256):
    with open(path, encoding="utf-8") as f:
        shelf = json.load(f)
    file = {"platform": platform, "name": name, "key": key, "size": int(size), "sha256": sha256}
    json.dump(shelved(shelf, version, file), sys.stdout, indent=2)
    print()


main(*sys.argv[1:8])

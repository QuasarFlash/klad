import json
import sys

import generate as g


def parts():
    paint = g.paint()
    stem = next(i for i, op in enumerate(paint) if op.get("color") == g.INK)
    relief = paint[-1]
    plate = g.ops()[:4]
    return {
        "plate": plate,
        "ember": [g.layer("paint", "paint"), *paint[:stem], relief],
        "ink": [g.layer("paint", "paint"), *paint[stem:-1], relief],
    }


def paths():
    return {"ember": g.along(g.ARM + g.LEG[1:]), "ink": g.along(g.STEM)}


if __name__ == "__main__":
    part = sys.argv[1] if len(sys.argv) > 1 else ""
    if part == "paths":
        print(json.dumps({k: [[round(x, 1), round(y, 1)] for x, y in v] for k, v in paths().items()}))
    elif part in parts():
        print("\n".join(json.dumps(op) for op in parts()[part]))
    else:
        sys.exit("hello.py plate|ember|ink|paths")

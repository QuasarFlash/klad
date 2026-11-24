import json
import sys

WIDTH, HEIGHT = 660, 400
PAPER = "#f4efe6"
INK = "#2b2a33"
APP, APPS = (170, 190), (490, 190)


def node(id, **fields):
    return {"op": "add", "node": {"id": id, **fields}, "in": "art", "frame": 0}


def ops():
    arrow_y = APP[1]
    start, end = APP[0] + 78, APPS[0] - 78
    return [
        {"op": "add", "layer": {"id": "art", "kind": "vector", "name": "art"}},
        node("paper", type="rect", x=0, y=0, w=WIDTH, h=HEIGHT, fill=PAPER),
        node("well-app", type="ellipse", cx=APP[0], cy=APP[1], rx=84, ry=84, fill="#ebe4d6"),
        node("well-apps", type="ellipse", cx=APPS[0], cy=APPS[1], rx=84, ry=84, fill="#ebe4d6"),
        node(
            "shaft",
            type="path",
            d=f"M{start} {arrow_y} C {start + 40} {arrow_y - 26} {end - 40} {arrow_y - 26} {end} {arrow_y}",
            fill="none",
            stroke={"paint": INK, "width": 5, "cap": "round"},
        ),
        node(
            "head",
            type="path",
            d=f"M{end - 18} {arrow_y - 15} L {end} {arrow_y} L {end - 20} {arrow_y + 11}",
            fill="none",
            stroke={"paint": INK, "width": 5, "cap": "round", "join": "round"},
        ),
        node(
            "say",
            type="text",
            x=WIDTH / 2,
            y=332,
            text="Drag klad into Applications",
            size=17,
            font="Inter",
            align="center",
            fill=INK,
        ),
    ]


if __name__ == "__main__":
    print("\n".join(json.dumps(op) for op in ops()))
    sys.exit(0)

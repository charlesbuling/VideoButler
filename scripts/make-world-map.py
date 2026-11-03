#!/usr/bin/env python3
"""Build vb-geo's offline world map from Natural Earth.

Reads three Natural Earth 1:10m layers (public domain) and writes, into
core/vb-geo/data/:

  world.json   every country, and China's provinces, as simplified outlines
               with Chinese and English names and a label point. The engine
               finds which region a GPS fix falls in from it, and the app
               draws the memory map from the same file, served by `geo.world`.
  places.tsv   populated places with Chinese and English names, for naming a
               trip after the city it stayed near.

Countries come from the China point-of-view layer
(ne_10m_admin_0_countries_chn): Taiwan is part of China there, and South Tibet
and Aksai Chin are drawn as China's. That is not a reviewed map: a map shown in
an app published in mainland China needs a 审图号, which this data does not
carry. See PRD.md (licensing and compliance).

Run through scripts/make-world-map.sh, which downloads the layers first.
Standard library only, so it runs on any Python 3.
"""

import json
import math
import sys
from pathlib import Path

SRC = Path(sys.argv[1] if len(sys.argv) > 1 else "build/natural-earth")
OUT = Path(sys.argv[2] if len(sys.argv) > 2 else "core/vb-geo/data")

# Degrees. Outlines are drawn on a world map and read for which region a
# point is in; a kilometre or two of coastline is neither.
COUNTRY_TOLERANCE = 0.04
PROVINCE_TOLERANCE = 0.025
# Islands smaller than this (square degrees) are dropped, except a country's
# largest piece, so small nations stay on the map.
MIN_RING_AREA = 0.02
SCALE = 100  # stored as hundredths of a degree

SHORT_ZH = {
    "中华人民共和国": "中国",
    "大韩民国": "韩国",
    "朝鲜民主主义人民共和国": "朝鲜",
    "蒙古国": "蒙古",
    "美利坚合众国": "美国",
}

PROVINCE_SHORT = {
    "CN-XJ": "新疆", "CN-XZ": "西藏", "CN-NM": "内蒙古", "CN-GX": "广西",
    "CN-NX": "宁夏",
}


def short_province(code, name_zh):
    if code in PROVINCE_SHORT:
        return PROVINCE_SHORT[code]
    for suffix in ("省", "市"):
        if name_zh.endswith(suffix):
            return name_zh[: -len(suffix)]
    return name_zh


def area(ring):
    s = 0.0
    for i in range(len(ring)):
        x1, y1 = ring[i]
        x2, y2 = ring[(i + 1) % len(ring)]
        s += x1 * y2 - x2 * y1
    return abs(s) / 2


def dp(points, tol):
    """Douglas-Peucker, iterative."""
    if len(points) < 3:
        return points
    keep = [False] * len(points)
    keep[0] = keep[-1] = True
    stack = [(0, len(points) - 1)]
    while stack:
        a, b = stack.pop()
        ax, ay = points[a]
        bx, by = points[b]
        dx, dy = bx - ax, by - ay
        norm = math.hypot(dx, dy)
        best, idx = -1.0, -1
        for i in range(a + 1, b):
            px, py = points[i]
            if norm == 0:
                d = math.hypot(px - ax, py - ay)
            else:
                d = abs(dy * px - dx * py + bx * ay - by * ax) / norm
            if d > best:
                best, idx = d, i
        if best > tol and idx > 0:
            keep[idx] = True
            stack.append((a, idx))
            stack.append((idx, b))
    return [p for p, k in zip(points, keep) if k]


def rings_of(geometry):
    """Outer rings only: holes are lakes and enclaves, and a lake drawn as
    land is a smaller error on a world map than a hole drawn as sea."""
    if geometry["type"] == "Polygon":
        return [geometry["coordinates"][0]]
    if geometry["type"] == "MultiPolygon":
        return [poly[0] for poly in geometry["coordinates"]]
    return []


def encode(rings, tol, keep_largest=True):
    rings = [r[:-1] if r[0] == r[-1] else r for r in rings if len(r) >= 4]
    if not rings:
        return []
    largest = max(rings, key=area)
    out = []
    for ring in rings:
        if area(ring) < MIN_RING_AREA and not (keep_largest and ring is largest):
            continue
        simple = dp(ring + [ring[0]], tol)[:-1]
        if len(simple) < 3:
            simple = ring if len(ring) <= 8 else ring[:: max(1, len(ring) // 8)]
        flat = []
        for x, y in simple:
            flat.append(round(x * SCALE))
            flat.append(round(y * SCALE))
        out.append(flat)
    return out


def main():
    a0 = json.load(open(SRC / "ne_10m_admin_0_countries_chn.geojson"))
    a1 = json.load(open(SRC / "ne_10m_admin_1_states_provinces.geojson"))
    pp = json.load(open(SRC / "ne_10m_populated_places.geojson"))

    regions = []
    taiwan_rings = []
    for f in a0["features"]:
        p = f["properties"]
        code = p["ISO_A2_EH"] if p["ISO_A2_EH"] not in ("-99", None) else p["ADM0_A3"]
        if p["NAME"] == "Scarborough Reef":
            code = "CN"
        rings = rings_of(f["geometry"])
        if code == "CN":
            # Taiwan is part of China in this layer; it is cut out here to
            # stand as a province beside the others.
            china = []
            for r in rings:
                xs = [x for x, _ in r]
                ys = [y for _, y in r]
                cx, cy = sum(xs) / len(xs), sum(ys) / len(ys)
                if 119.0 <= cx <= 122.5 and 21.5 <= cy <= 25.6 and max(xs) - min(xs) < 4:
                    taiwan_rings.append(r)
                else:
                    china.append(r)
            if p["NAME"] != "Scarborough Reef":
                rings = china
        zh = SHORT_ZH.get(p["NAME_ZH"], p["NAME_ZH"] or p["NAME"])
        en = p["NAME_EN"] or p["NAME"]
        if p["NAME"] == "Scarborough Reef":
            zh, en = "黄岩岛", "Huangyan Island"
            code = "CN-HY"
        regions.append({
            "id": code,
            "kind": "country" if code != "CN-HY" else "province",
            **({"parent": "CN"} if code == "CN-HY" else {}),
            "zh": zh,
            "en": en,
            "label": [round(p["LABEL_X"] * SCALE), round(p["LABEL_Y"] * SCALE)],
            "rings": encode(rings, COUNTRY_TOLERANCE),
        })

    for f in a1["features"]:
        p = f["properties"]
        if p["adm0_a3"] != "CHN" or not p["iso_3166_2"].startswith("CN-"):
            continue
        code = p["iso_3166_2"]
        if code.endswith("~"):
            code = "CN-" + "".join(c for c in p["name"] if c.isalpha())[:8].upper()
        zh = short_province(p["iso_3166_2"], p["name_zh"] or p["name"])
        regions.append({
            "id": code,
            "kind": "province",
            "parent": "CN",
            "zh": zh,
            "en": p["name_en"] or p["name"],
            "label": [round(p["longitude"] * SCALE), round(p["latitude"] * SCALE)],
            "rings": encode(rings_of(f["geometry"]), PROVINCE_TOLERANCE),
        })

    regions.append({
        "id": "CN-TW", "kind": "province", "parent": "CN",
        "zh": "台湾", "en": "Taiwan", "label": [12097, 2370],
        "rings": encode(taiwan_rings, PROVINCE_TOLERANCE),
    })
    # Hong Kong and Macao are their own features in the country layer; they
    # are China's special administrative regions, drawn as provinces are.
    for r in regions:
        if r["id"] in ("HK", "MO"):
            r["kind"] = "province"
            r["parent"] = "CN"
            r["id"] = "CN-" + r["id"]
    regions = [r for r in regions if r["rings"]]

    world = {
        "source": "Natural Earth 1:10m, public domain (naturalearthdata.com): "
                  "admin 0 countries from China's point of view, admin 1 "
                  "provinces, simplified. Not a reviewed map.",
        "scale": SCALE,
        "regions": regions,
    }
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "world.json").write_text(json.dumps(world, ensure_ascii=False, separators=(",", ":")))

    lines = ["# name_en\tname_zh\tlat\tlon\tpopulation\tcountry", ]
    for f in pp["features"]:
        p = f["properties"]
        country = p.get("ISO_A2") or ""
        if country in ("-99", None):
            country = ""
        if country == "TW":
            country = "CN"
        if country in ("HK", "MO"):
            country = "CN"
        en = (p.get("NAME_EN") or p.get("NAME") or "").replace("\t", " ")
        zh = (p.get("NAME_ZH") or "").replace("\t", " ")
        lines.append(f"{en}\t{zh}\t{p['LATITUDE']:.4f}\t{p['LONGITUDE']:.4f}\t{int(p.get('POP_MAX') or 0)}\t{country}")
    (OUT / "places.tsv").write_text("\n".join(lines) + "\n")

    size = (OUT / "world.json").stat().st_size
    points = sum(len(ring) // 2 for r in regions for ring in r["rings"])
    print(f"{len(regions)} regions, {points} points, {size / 1024:.0f} KB; {len(lines) - 1} places")


if __name__ == "__main__":
    main()

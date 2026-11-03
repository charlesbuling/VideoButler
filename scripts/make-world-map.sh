#!/usr/bin/env bash
# Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn>
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU Affero General Public License as
# published by the Free Software Foundation, either version 3 of the
# License, or (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU Affero General Public License for more details.

# The memory map's outlines and place names, from Natural Earth (public
# domain), into core/vb-geo/data/. Run after changing make-world-map.py;
# commit what it writes. Downloads resume, because this network drops them.
set -euo pipefail
cd "$(dirname "$0")/.."
SRC=build/natural-earth
BASE=https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson
mkdir -p "$SRC"
for f in ne_10m_admin_0_countries_chn.geojson ne_10m_admin_1_states_provinces.geojson \
         ne_10m_populated_places.geojson; do
  for attempt in 1 2 3 4 5 6 7 8 9 10; do
    curl -sS -m 300 -C - -o "$SRC/$f" "$BASE/$f" && break
    sleep 2
  done
  python3 -c "import json,sys; json.load(open(sys.argv[1]))" "$SRC/$f" \
    || { echo "$f did not download whole" >&2; exit 1; }
done
python3 scripts/make-world-map.py "$SRC" core/vb-geo/data

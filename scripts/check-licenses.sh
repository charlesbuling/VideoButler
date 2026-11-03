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

# Guard against the two dependencies that cannot ship.
#
# CoTracker3 and MMAudio are both CC-BY-NC. Neither is a Rust crate, so
# cargo-deny cannot see them; they would arrive as a model download, a Python
# sidecar, a vendored checkout or a manifest entry. A name appearing in real
# code or in a dependency manifest is the earliest signal available.
#
# Comment lines are ignored on purpose. The codebase documents why these are
# banned, and a check that fired on its own warnings would be switched off
# within a week.
#
# Apache-2.0 alternatives for the tracker: TAPIR, BootsTAPIR, LocoTrack.
set -uo pipefail

cd "$(dirname "$0")/.."

BANNED=("cotracker" "mmaudio")
status=0

for name in "${BANNED[@]}"; do
  hits=$(grep -rin "$name" . \
    --exclude-dir=.git \
    --exclude-dir=target \
    --exclude-dir=test-media \
    --exclude-dir=scripts \
    --include="*.rs" \
    --include="*.toml" \
    --include="*.json" \
    --include="*.yaml" \
    --include="*.yml" \
    --include="*.dart" \
    --include="*.py" \
    --include="*.txt" 2>/dev/null \
    | grep -vE ':[0-9]+:\s*(//|#|\*|///|//!)' || true)

  if [ -n "$hits" ]; then
    echo "BLOCKED: '$name' is CC-BY-NC and cannot ship. Found in:"
    echo "$hits" | sed 's/^/  /'
    status=1
  fi
done

if [ "$status" -eq 0 ]; then
  echo "No non-commercial dependencies found."
fi
exit "$status"

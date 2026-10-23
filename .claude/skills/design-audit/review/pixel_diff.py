#!/usr/bin/env python3
"""How much each QA capture changed between two runs, to check a UI change is
actually visible.

  python3 .claude/skills/design-audit/review/pixel_diff.py BEFORE_DIR AFTER_DIR [--json]

Prints, per capture present in both, the share of pixels whose largest
channel moved by more than 8, then a summary. Captures of different sizes
count as 100% changed.
"""
import json, os, sys

from PIL import Image, ImageChops

before, after = sys.argv[1], sys.argv[2]
rows = []
for name in sorted(os.listdir(before)):
    if not name.endswith('.png') or not os.path.exists(os.path.join(after, name)):
        continue
    a = Image.open(os.path.join(before, name)).convert('RGB')
    b = Image.open(os.path.join(after, name)).convert('RGB')
    if a.size != b.size:
        rows.append((name, 1.0))
        continue
    diff = ImageChops.difference(a, b).convert('L').point(lambda v: 255 if v > 8 else 0)
    rows.append((name, diff.histogram()[255] / (a.size[0] * a.size[1])))

rows.sort(key=lambda r: -r[1])
if '--json' in sys.argv:
    print(json.dumps(rows))
    sys.exit()
for name, share in rows:
    print(f'{share * 100:6.2f}%  {name}')
n = len(rows)
print(f'\n{n} captures: {sum(s == 0 for _, s in rows)} identical, '
      f'{sum(s < 0.005 for _, s in rows)} under 0.5%, {sum(s > 0.02 for _, s in rows)} over 2%')

"""Generate pinned Unicode search tables; inputs are explicit local UCD files.
No downloads. Run: python -X utf8 scripts/generate-unicode-search.py work/unicode-17
"""
from pathlib import Path
import hashlib
import sys

root = Path(__file__).resolve().parent.parent
source = Path(sys.argv[1])
hashes = {
    "CaseFolding.txt": "ff8d8fefbf123574205085d6714c36149eb946d717a0c585c27f0f4ef58c4183",
    "UnicodeData.txt": "2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c",
}
for name, expected in hashes.items():
    assert hashlib.sha256((source / name).read_bytes()).hexdigest() == expected, name

simple, full = [], []
for line in (source / "CaseFolding.txt").read_text(encoding="utf-8").splitlines():
    fields = line.split("#")[0].split(";")
    if len(fields) < 3 or fields[1].strip() not in ("C", "F"):
        continue
    code = int(fields[0], 16)
    mapping = [int(n, 16) for n in fields[2].split()]
    assert 1 <= len(mapping) <= 3
    if len(mapping) == 1:
        simple.append((code, mapping[0]))
    else:
        full.append((code, mapping + [0] * (3 - len(mapping)), len(mapping)))

words = {0x200C, 0x200D}
begin = None
for line in (source / "UnicodeData.txt").read_text(encoding="utf-8").splitlines():
    code, name, category, *_ = line.split(";")
    code = int(code, 16)
    if name.endswith(", First>"):
        begin = code
        continue
    start = begin if name.endswith(", Last>") else code
    begin = None
    if category[0] in "LMN" or category == "Pc":
        words.update(range(start, code + 1))
ranges = []
for code in sorted(words):
    if ranges and ranges[-1][1] + 1 == code:
        ranges[-1][1] = code
    else:
        ranges.append([code, code])

lines = [
    "// Generated from Unicode 17.0.0; do not edit. See scripts/generate-unicode-search.py.",
    "// Copyright © 2025 Unicode, Inc. Data under Unicode License V3.",
    "// License and original CaseFolding.txt: third_party/unicode-17/.",
    "#[rustfmt::skip]",
    "pub(super) const SIMPLE: &[(u32, u32)] = &[",
]
lines += [f"    (0x{a:X}, 0x{b:X})," for a, b in simple]
lines += ["];", "#[rustfmt::skip]", "pub(super) const FULL: &[(u32, [u32; 3], usize)] = &["]
lines += [f"    (0x{a:X}, [{', '.join(f'0x{b:X}' for b in bs)}], {n})," for a, bs, n in full]
lines += ["];", "#[rustfmt::skip]", "pub(super) const WORD: &[(u32, u32)] = &["]
lines += [f"    (0x{a:X}, 0x{b:X})," for a, b in ranges]
lines += ["];", ""]
(root / "crates/jc-text/src/unicode_search_data.rs").write_text("\n".join(lines), encoding="utf-8", newline="\n")
vendor = root / "third_party/unicode-17"
vendor.mkdir(parents=True, exist_ok=True)
for name in ("CaseFolding.txt", "LICENSE.txt"):
    (vendor / name).write_bytes((source / name).read_bytes())
print(f"Unicode 17: {len(simple)} single mappings, {len(full)} expansions, {len(ranges)} word ranges")

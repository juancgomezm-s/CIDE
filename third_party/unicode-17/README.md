# Unicode search data

Unicode 17.0.0 inputs, © 2025 Unicode, Inc.; Unicode License V3, retained in
LICENSE.txt. These are imported data, not a port of another editor's algorithm.
New streaming matching and byte-coordinate mapping are JC-IDE code.

- https://www.unicode.org/Public/17.0.0/ucd/CaseFolding.txt
  SHA-256 `ff8d8fefbf123574205085d6714c36149eb946d717a0c585c27f0f4ef58c4183`
- https://www.unicode.org/Public/17.0.0/ucd/UnicodeData.txt
  SHA-256 `2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c`

CaseFolding.txt is retained for exhaustive default-mapping conformance tests.
UnicodeData.txt is a generator input, not a runtime/repository dependency; obtain
the pinned file explicitly to regenerate. Run from the repository root:

    python -X utf8 scripts/generate-unicode-search.py path/to/local/ucd-inputs

The input folder also supplies LICENSE.txt. The script validates hashes and does
not access the network. Output: crates/jc-text/src/unicode_search_data.rs.

Default full folding uses C+F mappings and excludes Turkic T; it does not perform
normalization, accent stripping or source conversion. The editor word rule is
Unicode categories L/M/N/Pc plus U+200C/U+200D, not full UAX #29 segmentation.

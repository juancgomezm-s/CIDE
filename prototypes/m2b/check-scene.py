"""Small static validation of the visual fixture. Does not certify a GUI/backend."""
import json
from html.parser import HTMLParser
from pathlib import Path

folder = Path(__file__).resolve().parent
class SceneParser(HTMLParser):
    def __init__(self):
        super().__init__()
        self.ids = set()
        self.collect = False
        self.data = []
    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        assert tag not in {"input", "textarea", "iframe"}, tag
        assert "contenteditable" not in attrs
        identity = attrs.get("id")
        if identity:
            assert identity not in self.ids, identity
            self.ids.add(identity)
        for key in ("src", "href"):
            if key in attrs:
                assert "://" not in attrs[key], attrs[key]
                assert (folder / attrs[key]).resolve().exists(), attrs[key]
        self.collect = tag == "script" and identity == "scene-data"
    def handle_endtag(self, tag):
        if tag == "script":
            self.collect = False
    def handle_data(self, data):
        if self.collect:
            self.data.append(data)
parser = SceneParser()
parser.feed((folder / "index.html").read_text(encoding="utf-8"))
scene = json.loads("".join(parser.data))
assert scene["fixtureOnly"] and scene["schema"] == 1
assert len(scene["documents"]) == 2 and len(scene["views"]) == 2
sources = {source["id"]: source for source in scene["documents"]}
assert len(sources) == 2
for source in sources.values():
    assert source["encoding"] == "Windows-1252"
    assert source["eol"] == "CRLF" and source["finalNewline"] and not source["bom"]
    encoded = ("\r\n".join(source["lines"]) + "\r\n").encode("cp1252", errors="strict")
    assert encoded.endswith(b"\r\n")
    for hit in source["highlights"]:
        line = source["lines"][hit["line"]]
        assert line[hit["scalarStart"]:hit["scalarStart"] + hit["scalarLength"]] == scene["search"]["query"]
for view in scene["views"]:
    source = sources[view["documentId"]]
    cursor = view["cursor"]
    assert 0 <= cursor["line"] < len(source["lines"])
    assert 0 <= cursor["scalarColumn"] <= len(source["lines"][cursor["line"]])
assert scene["search"]["expectedRevision"] == sources[scene["search"]["documentId"]]["revision"]
assert scene["groups"][1]["pinned"] and not scene["scrollLinked"]
assert len(sources[1]["highlights"]) == 3
for required in ("workspace", "primary-source", "reference-source", "search-results", "palette", "scene-data"):
    assert required in parser.ids
print("Scene fixture validated: 2 documents, 2 views, 3 exact scalar highlights, explicit cp1252/CRLF, local assets, no editable source control.")

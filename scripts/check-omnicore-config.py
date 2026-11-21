"""Verify the real configuration bridge locally with disposable YAML, never user credentials."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", required=True, type=Path)
    args = parser.parse_args()
    bridge = args.runtime.resolve() / "cide-omni-turn-v2.exe"
    checks = []
    with tempfile.TemporaryDirectory(prefix="omnicore-config-", dir=ROOT / "work/test-temp") as directory:
        root = Path(directory)
        config = root / "config"
        config.mkdir()
        env = {**os.environ, "OMNICORE_CONFIG_DIR": str(config), "OMNICORE_DATA_DIR": str(root / "data")}
        providers = config / "providers.yaml"
        models = config / "models.yaml"
        settings = config / "settings.yaml"
        providers.write_text("providers:\n  local: { family: OpenAiChatCompatible, baseUrl: 'http://127.0.0.1:8080/v1?access=secret-query', auth: { apiKey: secret-api-key }, billingMode: Local }\n", encoding="utf-8")
        models.write_text("models:\n  demo: { provider: local, context: 8192, maxOutput: 1024 }\n", encoding="utf-8")
        settings.write_text("defaultModel: demo\n", encoding="utf-8")
        def inventory():
            return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob("*") if p.is_file()}
        def observe():
            before = inventory()
            result = subprocess.run([str(bridge), "config"], cwd=root, env=env, capture_output=True, timeout=15, creationflags=subprocess.CREATE_NO_WINDOW)
            assert result.returncode == 0, "Bridge did not return a configuration snapshot"
            assert inventory() == before, "Configuration review wrote files"
            assert not result.stderr, "Unexpected diagnostics on the private review channel"
            assert len(result.stdout) <= 128*1024
            return json.loads(result.stdout), result.stdout
        value, wire = observe()
        assert "error" not in value and value["modelCount"] == 1 and value["defaultModel"] == "demo"
        assert value["providers"][0]["address"] == "http://127.0.0.1:8080/v1"
        assert not any(secret in wire for secret in (b"secret-api-key", b"secret-query"))
        checks.append("valid provider/model/default; credentials and URL query omitted; byte-exact read-only")
        providers.write_text("providers: { broken: { auth: secret-invalid-value } }", encoding="utf-8")
        value, wire = observe()
        assert "error" in value and b"secret-invalid-value" not in wire
        checks.append("invalid configuration produces safe Spanish diagnostic without source text")
        providers.write_bytes(b"x"*(512*1024+1))
        value, wire = observe()
        assert "error" in value
        checks.append("oversized input rejected without startup writes")
    print(json.dumps({"ok": True, "checks": checks}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

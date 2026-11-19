"""Validate the empty CIDE GUI against OmniCore and a fake local model."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import runpy
import shutil
import socket
import subprocess
from tempfile import NamedTemporaryFile, TemporaryDirectory
from threading import Thread


ROOT = Path(__file__).resolve().parents[1]
QUESTION = "¿Consulta inicial desde CIDE?"
ANSWER = "Respuesta sintética desde OmniCore"
TEST_FILTER = "omnicore_empty_editor_connects"


class Model(BaseHTTPRequestHandler):
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if self.path != "/v1/chat/completions" or request.get("model") != "cide-fixture":
            self.send_error(400)
            return
        self.server.questions.append(request.get("messages", []))
        event = {"choices": [{"delta": {"content": ANSWER}, "finish_reason": "stop"}]}
        body = f"data: {json.dumps(event)}\n\ndata: [DONE]\n\n".encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--omnicore", type=Path, required=True)
    parser.add_argument("--bundled", action="store_true", help="discover OmniCore beside the test executable in a workspace extraction")
    args = parser.parse_args()
    executable = args.omnicore.resolve(strict=True)
    if args.bundled and (not executable.is_relative_to(ROOT) or executable.parts[-2:] != ("omnicore", "omni.exe")):
        parser.error("--bundled requiere el bin/omnicore/omni.exe de una extracción del MSI dentro del proyecto.")
    local_runner = ROOT / "scripts/check-local.py"
    if local_runner.is_file():
        cargo, env = runpy.run_path(str(local_runner))["environment"]()
    else:
        cargo = shutil.which("cargo")
        if cargo is None:
            parser.error("Se requiere Cargo para ejecutar la prueba local.")
        temporary = ROOT / "work/test-temp"
        temporary.mkdir(parents=True, exist_ok=True)
        env = {**os.environ, "TEMP": str(temporary), "TMP": str(temporary)}
    with TemporaryDirectory(prefix="cide-omnicore-", dir=env["TEMP"]) as directory:
        root = Path(directory)
        with ThreadingHTTPServer(("127.0.0.1", 0), Model) as model, socket.socket() as offline:
            offline.bind(("127.0.0.1", 0))
            offline_url = f"http://127.0.0.1:{offline.getsockname()[1]}/v1"
            model.questions = []
            config = root / "config"
            config.mkdir()
            (root / "local-app-data").mkdir()
            (config / "providers.yaml").write_text(
                "providers:\n  cide-fixture:\n    family: OpenAiChatCompatible\n"
                f"    baseUrl: http://127.0.0.1:{model.server_port}/v1\n"
                "    auth: none\n    billingMode: Local\n"
                "  cide-sin-servicio:\n    family: OpenAiChatCompatible\n"
                f"    baseUrl: {offline_url}\n"
                "    auth: none\n    billingMode: Local\n", encoding="utf-8",
            )
            (config / "models.yaml").write_text(
                "models:\n  cide-fixture:\n    provider: cide-fixture\n"
                "    context: 32768\n    maxOutput: 2048\n"
                "  alternativo-local:\n    provider: cide-sin-servicio\n"
                "    context: 8192\n    maxOutput: 2048\n", encoding="utf-8",
            )
            for name in ("OMNI_QWEN_KEY", "OMNI_BASE_URL", "OMNICORE_TRUSTED_ACTION_BOOTSTRAP"):
                env.pop(name, None)
            for name in ("OMNICORE_AGENT_HOME", "CODEX_HOME", "CLAUDE_CONFIG_DIR", "OPENCODE_CONFIG_DIR", "XDG_CONFIG_HOME"):
                env[name] = str(root / "agents" / name)
            env.update(
                LOCALAPPDATA=str(root / "local-app-data"), CIDE_NO_SESSION="1",
                CIDE_OMNICORE_EXE=str(executable), OMNICORE_CONFIG_DIR=str(config),
                OMNICORE_DATA_DIR=str(root / "data"), OMNI_MODEL="cide-fixture",
                OMNI_BASE_URL=offline_url,
                NO_PROXY="127.0.0.1", CARGO_TARGET_DIR=str(ROOT / "target/embedded-git-next"),
            )
            thread = Thread(target=model.serve_forever, daemon=True)
            thread.start()
            try:
                run_test(cargo, env, executable.parent.parent if args.bundled else None)
                requests = ["\n".join(str(message.get("content", "")) for message in messages)
                            for messages in model.questions]
                assert len(requests) == 4 and QUESTION in requests[0]
                assert all(text in requests[1] for text in (
                    QUESTION, ANSWER, "Modo Plan", "ejemplo.p", "México", "contexto.",
                ))
                assert "Confirma la consulta sin contexto." in requests[2]
                assert "otro.p" not in requests[2] and "Solo archivo activo" not in requests[2]
                assert all(text in requests[3] for text in (
                    "Confirma la consulta con contexto.", "otro.p", "Solo archivo activo",
                ))
                assert requests[3].count("/* contexto de prueba */") == 512
                print("La GUI eligió otro proveedor, envió el archivo activo y respetó el interruptor de contexto.")
            finally:
                model.shutdown()
                thread.join()


def run_test(cargo, env, bundled_dir):
    args = [cargo, "test", "-p", "jc-gui", "--lib", "--locked", "--offline"]
    if bundled_dir is None:
        subprocess.run([*args, TEST_FILTER, "--", "--ignored"], cwd=ROOT, env=env, check=True, timeout=240)
        return
    env.pop("CIDE_OMNICORE_EXE", None)
    output = subprocess.run([*args, "--no-run", "--message-format=json"], cwd=ROOT, env=env,
                            stdout=subprocess.PIPE, check=True, timeout=240).stdout
    artifacts = [json.loads(line) for line in output.splitlines()]
    binary, = [Path(item["executable"]) for item in artifacts if item.get("executable") and item["target"]["name"] == "jc_gui"]
    with NamedTemporaryFile(prefix="cide-test-", suffix=".exe", dir=bundled_dir, delete=False) as temporary:
        copy = Path(temporary.name)
    try:
        shutil.copyfile(binary, copy)
        subprocess.run([copy, TEST_FILTER, "--ignored"], cwd=ROOT, env=env, check=True, timeout=60)
    finally:
        copy.unlink()


if __name__ == "__main__":
    main()

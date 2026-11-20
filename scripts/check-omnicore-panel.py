"""Validate CIDE against OmniCore and a deterministic local model."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import runpy
import shutil
import socket
import subprocess
import sys
from tempfile import NamedTemporaryFile, TemporaryDirectory
from threading import Thread


ROOT = Path(__file__).resolve().parents[1]
QUESTION = "¿Consulta inicial desde CIDE?"
ANSWER = "Respuesta sintética desde OmniCore"
TEST_FILTER = "omnicore_empty_editor_connects"

LSP_FIXTURE = """
import json
import sys

while header := sys.stdin.buffer.readline():
    headers = {}
    while header.strip():
        key, value = header.decode("ascii").split(":", 1)
        headers[key.lower()] = value.strip()
        header = sys.stdin.buffer.readline()
    request = json.loads(sys.stdin.buffer.read(int(headers["content-length"])))
    if "id" not in request:
        continue
    result = None
    if request["method"] == "initialize":
        result = {"capabilities": {"completionProvider": {}, "textDocumentSync": 1}}
    elif request["method"] == "textDocument/completion":
        result = [{"label": "subtotal", "detail": "i32",
                   "documentation": "private_lsp_documentation",
                   "insertText": "private_lsp_edit"}]
    response = json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(response)}\\r\\n\\r\\n".encode() + response)
    sys.stdout.buffer.flush()
"""


class Model(BaseHTTPRequestHandler):
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if self.path != "/v1/chat/completions" or request.get("model") != "cide-fixture":
            self.send_error(400)
            return
        messages = request.get("messages", [])
        self.server.questions.append(messages)
        selection_prompt = any("Cambia únicamente el literal seleccionado." in str(message.get("content", ""))
                               for message in messages)
        completion_prompt = any("Complete the code exactly at the cursor" in str(message.get("content", ""))
                                for message in messages)
        block_prompt = any("Complete the routine body at the cursor" in str(message.get("content", ""))
                           for message in messages)
        method_prompt = any("ABL method" in str(message.get("content", "")) for message in messages)
        segment_prompt = any("Complete only the control segment at the cursor" in str(message.get("content", ""))
                             for message in messages)
        abl_loop = any("ABL loop" in str(message.get("content", "")) for message in messages)
        rust_expression = any('"kind":"Rust match expression arm"' in str(message.get("content", ""))
                              for message in messages)
        abl_statement = any('"kind":"ABL single-statement branch"' in str(message.get("content", ""))
                            for message in messages)
        prompt = "\n".join(str(message.get("content", "")) for message in messages)
        if segment_prompt and "777 =>" in prompt:
            answer = json.dumps({"insertion": "{\n            let output = subtotal * 2;\n            output\n        }"})
        elif segment_prompt and "IF semanticAbl THEN" in prompt:
            insertion = "me\n    WITH NO-LABELS" if "negocio.customer.na" in prompt else "DISPLAY amount\n    WITH NO-LABELS"
            answer = json.dumps({"insertion": insertion})
        elif segment_prompt and rust_expression:
            answer = json.dumps({"insertion": "{\n            let output = n + 1;\n            output\n        }"})
        elif segment_prompt and abl_statement:
            answer = json.dumps({"insertion": "DISPLAY db.Customer.Name\n    db.Customer.Active"})
        elif segment_prompt and abl_loop:
            answer = json.dumps({"insertion": "DISPLAY db.Customer.Active.\n    NEXT."})
        elif segment_prompt:
            answer = json.dumps({"insertion": "let doubled = n * 2;\n            doubled"})
        elif block_prompt and method_prompt:
            answer = json.dumps({"insertion": "RETURN n + 1."})
        elif block_prompt:
            answer = json.dumps({"insertion": "let doubled = x * 2;\n    doubled"})
        elif completion_prompt:
            answer = json.dumps({"insertion": "valor "})
        elif selection_prompt:
            answer = json.dumps({"replacement": "té"}, ensure_ascii=False)
        else:
            answer = ANSWER
        event = {"choices": [{"delta": {"content": answer}, "finish_reason": "stop"}]}
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
    parser.add_argument("--rust-analyzer", type=Path)
    parser.add_argument("--openedge", type=Path, help="OpenEdge root containing bin and sports2000.db")
    parser.add_argument("--bundled", action="store_true", help="discover OmniCore beside the test executable in a workspace extraction")
    args = parser.parse_args()
    if bool(args.rust_analyzer) != bool(args.openedge):
        parser.error("--rust-analyzer y --openedge deben usarse juntos")
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
        fixture = root / "semantic-project"
        fixture.mkdir()
        if args.rust_analyzer:
            rust_binary = args.rust_analyzer.resolve(strict=True)
            lsp = {"command": str(rust_binary), "initializationOptions": {
                "cargo": {"offline": True}, "checkOnSave": False,
            }}
            (fixture / "Cargo.toml").write_text(
                '[package]\nname = "semantic_fixture"\nversion = "0.0.0"\nedition = "2021"\n'
                '[lib]\npath = "semantic.rs"\n', encoding="utf-8",
            )
            prepare_openedge(fixture, args.openedge.resolve(strict=True), env)
            env["CIDE_OMNICORE_REAL_SEMANTICS"] = "1"
        else:
            env.pop("CIDE_OMNICORE_REAL_SEMANTICS", None)
            lsp_stub = root / "lsp-fixture.py"
            lsp_stub.write_text(LSP_FIXTURE, encoding="utf-8")
            lsp = {"command": sys.executable, "args": ["-u", str(lsp_stub)]}
        lsp_config = root / "lsp.json"
        lsp_config.write_text(json.dumps({"servers": {"rust": lsp}}), encoding="utf-8")
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
                CIDE_LSP_CONFIG=str(lsp_config), CIDE_OMNICORE_SEMANTIC_FIXTURE=str(fixture),
                NO_PROXY="127.0.0.1",
            )
            env.setdefault("CARGO_TARGET_DIR", str(ROOT / "target/embedded-git-next"))
            thread = Thread(target=model.serve_forever, daemon=True)
            thread.start()
            try:
                run_test(cargo, env, executable.parent.parent if args.bundled else None)
                requests = ["\n".join(str(message.get("content", "")) for message in messages)
                            for messages in model.questions]
                assert len(requests) == 14 and QUESTION in requests[0]
                assert all(text in requests[1] for text in (
                    QUESTION, ANSWER, "Modo Plan", "ejemplo.p", "México", "contexto.",
                ))
                assert "Confirma la consulta sin contexto." in requests[2]
                assert "otro.p" not in requests[2] and "Solo archivo activo" not in requests[2]
                assert all(text in requests[3] for text in (
                    "Confirma la consulta con contexto.", "otro.p", "Solo archivo activo",
                ))
                assert requests[3].count("/* contexto de prueba */") == 512
                assert "Cambia únicamente el literal seleccionado." in requests[4] and "café" in requests[4]
                assert "inicio " not in requests[4] and "Solo archivo activo" not in requests[4]
                assert "Complete the code exactly at the cursor" in requests[5]
                assert "let café = " in requests[5] and "+ 2;" in requests[5]
                assert "Cambia únicamente" not in requests[5] and "Solo archivo activo" not in requests[5]
                assert "fn total(x: i32) -> i32 {" in requests[6]
                assert '"existingCloser":"}"' in requests[6]
                assert "METHOD PUBLIC INTEGER calcular(INPUT n AS INTEGER):" in requests[7]
                assert '"existingCloser":"END METHOD."' in requests[7]
                assert '"kind":"Rust match arm"' in requests[8] and "1 => {" in requests[8]
                assert "sibling_sentinel" not in requests[8]
                assert '"kind":"ABL loop"' in requests[9] and "FOR EACH db.Customer:" in requests[9]
                assert "sentinel_after_loop" not in requests[9]
                assert '"kind":"Rust match expression arm"' in requests[10] and '"existingCloser":","' in requests[10]
                assert "bare_rust_sibling" not in requests[10]
                assert '"kind":"ABL single-statement branch"' in requests[11] and '"existingCloser":"."' in requests[11]
                assert "bare_abl_sibling" not in requests[11]
                for request, provider, symbol, type_name, sibling in (
                    (requests[12], "LSP", "subtotal", "i32", "semantic_rust_sibling"),
                    (requests[13], "ABL", "name" if args.openedge else "amount",
                     "CHARACTER" if args.openedge else "DECIMAL", "semantic_abl_sibling"),
                ):
                    context = json.loads(next(line for line in request.splitlines() if line.startswith("{")))
                    assert any(
                        row["provider"] == provider
                        and row["symbol"].casefold() == symbol.casefold()
                        and (row["typeOrSignature"] or "").casefold() == type_name.casefold()
                        for row in context.get("semanticContext", [])
                    ), context
                    assert sibling not in request
                    assert "private_lsp_" not in request
                    print(f"{provider}: {symbol} ({type_name})")
                print("La GUI verificó el contexto semántico, el diff y Ctrl+Z en segmentos con y sin bloque.")
            finally:
                model.shutdown()
                thread.join()


def prepare_openedge(fixture, openedge, env):
    database = fixture / "sports2000"
    subprocess.run(
        [openedge / "bin/_dbutil.exe", "procopy", openedge / "sports2000", database],
        cwd=fixture, env={**env, "DLC": str(openedge)}, check=True, timeout=40,
    )
    config = fixture / ".cide"
    config.mkdir()
    (config / "progress.local.ini").write_text(
        f"executable={(openedge / 'bin/_progres.exe').as_posix()}\n"
        "cpinternal=iso8859-1\ncpstream=ibm850\nversion=11.7\n"
        "pf=.cide/desarrollo.local.pf\naliases=negocio:devlogica\n", encoding="utf-8",
    )
    (config / "desarrollo.local.pf").write_text(
        f'-db "{database.as_posix()}" -ld devlogica -1 -RO\n', encoding="utf-8",
    )


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

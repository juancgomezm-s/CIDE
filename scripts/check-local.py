"""Run CI-equivalent checks locally, offline, without installing anything."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
STEPS = {
    "fmt": ["fmt", "--all", "--", "--check"],
    "test": ["test", "--workspace", "--locked", "--offline"],
    "clippy": ["clippy", "--workspace", "--all-targets", "--locked", "--offline", "--", "-D", "warnings"],
    "build": ["build", "--workspace", "--release", "--locked", "--offline"],
    "demo": ["run", "--locked", "--offline", "-p", "jc-cli", "--", "demo"],
}


def environment():
    env = os.environ.copy()
    local = ROOT / "work/toolchain"
    rustup = local / "cargo/bin/rustup.exe"
    if sys.platform == "win32" and rustup.is_file():
        env.update(CARGO_HOME=str(local / "cargo"), RUSTUP_HOME=str(local / "rustup"))
    else:
        rustup = shutil.which("rustup")
    if not rustup:
        raise RuntimeError("No installed rustup found; no toolchain was installed")
    channel = tomllib.loads((ROOT / "rust-toolchain.toml").read_text(encoding="utf-8"))["toolchain"]["channel"]
    env.update(RUSTUP_AUTO_INSTALL="0", RUSTUP_TOOLCHAIN=channel, CARGO_NET_OFFLINE="true", CARGO_INCREMENTAL="0")
    # `which` only locates an installed toolchain. Direct binaries avoid rustup
    # proxies trying to provision missing components while a check is running.
    result = subprocess.run([str(rustup), "which", "--toolchain", channel, "cargo"],
                            cwd=ROOT, env=env, check=True, capture_output=True, text=True)
    cargo = Path(result.stdout.strip())
    if not cargo.is_file():
        raise RuntimeError("Pinned Cargo is not installed")
    env["PATH"] = str(cargo.parent) + os.pathsep + env.get("PATH", "")
    temp = ROOT / "work/test-temp"
    temp.mkdir(parents=True, exist_ok=True)
    env.update(TEMP=str(temp), TMP=str(temp), TMPDIR=str(temp))
    return str(cargo), env


def run_checks(steps, jobs, dry_run=False):
    if dry_run:
        for step in steps:
            print(f"{step}: cargo {' '.join(STEPS[step])}")
        print(f"Local only; {jobs} build jobs; no downloads or GitHub calls. Not a validation pass.")
        return 0
    cargo, env = environment()
    env["CARGO_BUILD_JOBS"] = str(jobs)
    for step in steps:
        cache = "build-cache" if step == "build" else "embedded-git-next"
        env["CARGO_TARGET_DIR"] = str(ROOT / "target" / cache)
        print(f"[{step}] cargo {' '.join(STEPS[step])}", flush=True)
        result = subprocess.run([cargo, *STEPS[step]], cwd=ROOT, env=env)
        if result.returncode:
            print(f"FAILED: {step} (exit {result.returncode}); later steps not run.", file=sys.stderr)
            return result.returncode
    print("Passed selected local checks: " + ", ".join(steps))
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--steps", nargs="+", choices=STEPS, default=list(STEPS))
    parser.add_argument("--jobs", type=int, choices=range(1, 5), default=2)
    parser.add_argument("--dry-run", action="store_true", help="Print commands without running tools")
    args = parser.parse_args()
    try:
        return run_checks(args.steps, args.jobs, args.dry_run)
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Local validation blocked: {error}. No remote fallback or installation attempted.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

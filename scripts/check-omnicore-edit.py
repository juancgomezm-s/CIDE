"""Verify native edit admission and routing locally, without invoking a model."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', required=True, type=Path)
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    if not runtime.is_relative_to(ROOT):
        parser.error('Usa una copia de OmniCore dentro de CIDE.')
    cache = ROOT / 'work/omnicore-edit-check'
    cache.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(['dotnet', 'build', str(ROOT / 'build_support/omnicore-edit-check/Check.csproj'),
                             '-c', 'Release', '-o', str(cache / 'out'),
                             f'-p:BaseIntermediateOutputPath={cache / "obj"}{os.sep}',
                             '--ignore-failed-sources'], capture_output=True, timeout=120)
    (cache / 'build.stdout').write_bytes(result.stdout)
    (cache / 'build.stderr').write_bytes(result.stderr)
    result.check_returncode()
    run = cache / str(time.time_ns())
    checks = []
    for billing, selected, allowed in [('Local', 'prueba', True), ('IncludedQuota', 'prueba', True),
                                      ('MeteredCurrency', 'prueba', False), ('Local', 'ausente', False)]:
        case = run / f'{billing}-{selected}'
        config = case / 'config'
        workspace = case / 'workspace'
        config.mkdir(parents=True)
        workspace.mkdir()
        (config / 'providers.yaml').write_text('providers:\n  local: { family: OpenAiChatCompatible, '
                                             'baseUrl: http://127.0.0.1:1/v1, auth: none, '
                                             f'billingMode: {billing}' + ' }\n', encoding='utf-8')
        (config / 'models.yaml').write_text('models:\n  prueba: { provider: local, context: 8192, maxOutput: 1024 }\n', encoding='utf-8')
        (config / 'settings.yaml').write_text('defaultModel: prueba\n', encoding='utf-8')
        def inventory():
            return {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in config.iterdir()}
        before = inventory()
        env = {**os.environ, 'OMNICORE_CONFIG_DIR': str(config), 'OMNICORE_DATA_DIR': str(case / 'data'),
               'LOCALAPPDATA': str(case / 'local'), 'APPDATA': str(case / 'roaming'), 'OMNI_MODEL': selected}
        result = subprocess.run([str(cache / 'out/Check.exe'), str(runtime), str(workspace), 'allow' if allowed else 'deny'],
                                env=env, capture_output=True, timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)
        (case / 'result.stdout').write_bytes(result.stdout)
        (case / 'result.stderr').write_bytes(result.stderr)
        result.check_returncode()
        assert inventory() == before, 'Configuration changed'
        assert json.loads(result.stdout)['ok']
        checks.append(f'{billing}/{selected}: ' + ('model bound, ScopedCoder' if allowed else 'rejected before Run'))
    print(json.dumps({'ok': True, 'provider_invoked': False, 'checks': checks, 'output': str(run)}, ensure_ascii=False))


if __name__ == '__main__':
    main()

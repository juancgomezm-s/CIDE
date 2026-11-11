"""Build a Windows MSI from the verified payload of package-cide.py."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import subprocess
from tempfile import TemporaryDirectory
import zipfile


ROOT = Path(__file__).resolve().parents[1]
RUNTIME_FILES = {
    'LICENSE', 'LICENSING.md',
    'assets/branding/cide-gui.ico', 'assets/branding/cide-tui.ico',
    'assets/branding/provenance.json', 'docs/GIT_EMBEDDED_LICENSES.md',
    *(f'bin/{name}.exe' for name in (
        'cide', 'cide-gui', 'cide-tui', 'jc-ide', 'jc-ide-gui', 'jc-ide-tui',
    )),
}


def prepare_payload(package, payload):
    with zipfile.ZipFile(package) as archive:
        manifest = json.loads(archive.read('manifest.json'))
        if (manifest['product'], manifest['version'], manifest['platform']) != (
            'CIDE', '0.1.0', 'Windows MSVC x64',
        ):
            raise ValueError('El paquete debe corresponder a CIDE 0.1.0 para Windows x64.')
        installed = set()
        for entry in manifest['files']:
            name = entry['path']
            if name not in RUNTIME_FILES and not name.startswith('third_party/'):
                continue
            parts = PurePosixPath(name).parts
            if any(
                part in ('.', '..') or any(char in part for char in '\\:/')
                or part.endswith(('.', ' ')) or PureWindowsPath(part).is_reserved()
                for part in parts
            ) or name != '/'.join(parts):
                raise ValueError(f'Ruta de paquete no válida: {name}')
            data = archive.read(name)
            if len(data) != entry['bytes'] or hashlib.sha256(data).hexdigest() != entry['sha256']:
                raise ValueError(f'Contenido de paquete alterado: {name}')
            destination = payload.joinpath(*parts)
            destination.parent.mkdir(parents=True, exist_ok=True)
            with destination.open('xb') as output:
                output.write(data)
            installed.add(name)
        if not RUNTIME_FILES <= installed or 'third_party/gitoxide/DEPENDENCIES.json' not in installed:
            raise ValueError('El paquete no contiene todos los binarios y avisos requeridos.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', required=True, type=Path)
    parser.add_argument('--wix', required=True, type=Path)
    parser.add_argument('--ui-extension', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    output = args.output.absolute()
    if os.name != 'nt' or output.suffix.lower() != '.msi' or os.path.lexists(output):
        parser.error('Se requiere Windows y una ruta MSI nueva.')
    with TemporaryDirectory(prefix='cide-msi-', dir=output.parent) as directory:
        work = Path(directory)
        payload = work / 'payload'
        prepare_payload(args.package, payload)
        license_text = (payload / 'LICENSE').read_text(encoding='utf-8')
        for character in ('\\', '{', '}'):
            license_text = license_text.replace(character, '\\' + character)
        license_rtf = work / 'license.rtf'
        license_rtf.write_text(
            r'{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\f0\fs20 '
            + license_text.replace('\n', '\\par\n') + '}', encoding='ascii',
        )
        candidate = work / output.name
        subprocess.run([
            str(args.wix.resolve(strict=True)), 'build', '-arch', 'x64', '-culture', 'es-ES',
            '-ext', str(args.ui_extension.resolve(strict=True)), '-d', f'Payload={payload}',
            '-d', f'LicenseRtf={license_rtf}', '-out', str(candidate), '-pdbtype', 'none',
            str(ROOT / 'installer/CIDE.wxs'),
        ], cwd=work, check=True)
        candidate.rename(output)
    print(output)


if __name__ == '__main__':
    main()

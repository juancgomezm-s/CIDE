"""Build a Windows MSI from the verified payload of package-cide.py."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import runpy
import shutil
import subprocess
from tempfile import TemporaryDirectory
import zipfile


ROOT = Path(__file__).resolve().parents[1]
OMNICORE_REQUIRED = runpy.run_path(str(ROOT / 'scripts/package-cide.py'))['OMNICORE_REQUIRED']
RUNTIME_FILES = {
    'LICENSE', 'LICENSING.md',
    'assets/branding/cide-gui.ico', 'assets/branding/cide-tui.ico',
    'assets/branding/provenance.json', 'docs/GIT_EMBEDDED_LICENSES.md',
    *(f'bin/{name}.exe' for name in (
        'cide', 'cide-gui', 'cide-tui', 'jc-ide', 'jc-ide-gui', 'jc-ide-tui',
    )),
}

def msi_version(value):
    if re.fullmatch(r"(?:0|[1-9][0-9]{0,4})(?:\.(?:0|[1-9][0-9]{0,4})){2}", value):
        major, minor, build = map(int, value.split('.'))
        if major <= 255 and minor <= 255 and build <= 65535:
            return value
    raise argparse.ArgumentTypeError('Usa una versión MSI de tres campos con límites 255.255.65535.')


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
            if name.startswith('bin/omnicore/'):
                if manifest.get('omnicore') is not True:
                    raise ValueError('El paquete contiene un componente OmniCore no declarado.')
            elif name not in RUNTIME_FILES and not name.startswith('third_party/'):
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
        if manifest.get('omnicore') is True and not {
            f'bin/omnicore/{name}' for name in OMNICORE_REQUIRED
        } <= installed:
            raise ValueError('El paquete OmniCore está incompleto.')


def build_shortcut_icons(payload, output, rc, link):
    # Windows Installer publishes advertised shortcut icons from separate PE files
    # with the same extension as the target. These PE files contain only resources.
    output.mkdir()
    for frontend, stem in (('gui', 'CideGuiIcon'), ('tui', 'CideTuiIcon')):
        icon = output / f'cide-{frontend}.ico'
        shutil.copyfile(payload / 'assets' / 'branding' / icon.name, icon)
        script = output / f'{stem}.rc'
        resource = output / f'{stem}.res'
        script.write_text(f'101 ICON "{icon.name}"\n', encoding='ascii')
        subprocess.run([
            str(rc), '/nologo', '/fo', resource.name, script.name,
        ], cwd=output, check=True)
        subprocess.run([
            str(link), '/nologo', '/dll', '/noentry', '/nodefaultlib',
            '/machine:x64', '/manifest:no', '/incremental:no', '/brepro',
            f'/out:{stem}.exe', resource.name,
        ], cwd=output, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', required=True, type=Path)
    parser.add_argument('--wix', required=True, type=Path)
    parser.add_argument('--ui-extension', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--rc', type=Path, default=shutil.which('rc.exe'),
                        help='Compilador de recursos de Windows SDK.')
    parser.add_argument('--link', type=Path, default=shutil.which('link.exe'),
                        help='Enlazador de MSVC para x64.')
    parser.add_argument('--msi-version', default='0.1.0', type=msi_version)
    parser.add_argument('--allow-same-version-upgrade', action='store_true',
                        help='Permite reemplazar una compilación anterior de la misma versión MSI.')
    args = parser.parse_args()
    output = args.output.absolute()
    if os.name != 'nt' or output.suffix.lower() != '.msi' or os.path.lexists(output):
        parser.error('Se requiere Windows y una ruta MSI nueva.')
    if args.rc is None or args.link is None:
        parser.error('Usa una consola de desarrollo x64 o indica --rc y --link.')
    rc, link = (path.resolve(strict=True) for path in (args.rc, args.link))
    with TemporaryDirectory(prefix='cide-msi-', dir=output.parent) as directory:
        work = Path(directory)
        payload = work / 'payload'
        prepare_payload(args.package, payload)
        icons = work / 'icons'
        build_shortcut_icons(payload, icons, rc, link)
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
            '-d', f'MsiVersion={args.msi_version}',
            '-d', 'AllowSameVersionUpgrade=' + ('yes' if args.allow_same_version_upgrade else 'no'),
            '-d', f'ShortcutIcons={icons}',
            '-d', f'LicenseRtf={license_rtf}', '-out', str(candidate), '-pdbtype', 'none',
            str(ROOT / 'installer/CIDE.wxs'),
        ], cwd=work, check=True)
        candidate.rename(output)
    print(output)


if __name__ == '__main__':
    main()

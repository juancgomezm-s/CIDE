"""Own ConPTY flows and rasterized VT-cell evidence; no personal clipboard."""
import os,argparse,ctypes as c,datetime as dt,importlib.util,json,uuid
from ctypes import wintypes as w
from pathlib import Path
spec=importlib.util.spec_from_file_location('owned_conpty',Path(__file__).with_name('smoke-explorer-conpty.py'))
base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
def snapshot(session,path):
    from PIL import Image,ImageDraw,ImageFont
    with session.sync:
        cells=[row[:] for row in session.cells];inks=[row[:] for row in session.inks]
    cw,ch=10,21;image=Image.new('RGB',(session.width*cw,session.height*ch),(6,24,34));draw=ImageDraw.Draw(image)
    normal=ImageFont.truetype('C:/Windows/Fonts/consola.ttf',16);bold=ImageFont.truetype('C:/Windows/Fonts/consolab.ttf',16)
    for y,row in enumerate(cells):
        for x,char in enumerate(row):
            fg,bg,strong,underline=inks[y][x];draw.rectangle((x*cw,y*ch,(x+1)*cw,(y+1)*ch),fill=bg)
            draw.text((x*cw,y*ch),char,font=bold if strong else normal,fill=fg)
            if underline:draw.line((x*cw,y*ch+18,(x+1)*cw-1,y*ch+18),fill=fg)
    image.save(path)
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--exe',type=Path);args=parser.parse_args()
    root=Path(__file__).resolve().parents[1];exe=(args.exe or root/'target/release/cide-tui.exe').resolve(strict=True)
    assert exe.is_relative_to(root) and exe.name=='cide-tui.exe'
    parent=root/'work/spanish-conpty';parent.mkdir(exist_ok=True)
    assert parent.resolve().is_relative_to(root) and not parent.is_symlink() and not getattr(parent.lstat(),'st_file_attributes',0)&0x400
    output=parent/(dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-')+uuid.uuid4().hex[:8]);output.mkdir()
    source=output/'README.md';original=('# CIDE: edición en español\r\n\r\n## Una interfaz para leer y programar\n\n'
        'Texto con **énfasis**, `código` y [un enlace inerte](https://example.invalid).\n\n'
        '- [x] Menús con letras Alt subrayadas\n- [ ] Navegación lateral con ayuda visible\n\n'
        '> La vista previa conserva los bytes originales.\n\n```rust\nfn saludo() {\n    println!("Hola, mundo");\n}\n```\n').encode('utf8')
    with source.open('xb') as f:f.write(original)
    session=None;checks=[];stage='startup';report={'ok':False,'physical_display_verified':False,'os_clipboard_verified':False,'avm_started':False,'raster_scope':'decoded VT cells and emitted RGB/attributes, not a physical terminal screenshot'}
    try:
        environment=os.environ.copy();environment.pop('NO_COLOR',None)
        session=base.Session(root,exe,(exe.parent/'examples/f3_console_host.exe').resolve(strict=True),source,environment=environment)
        session.wait(lambda:session.contains('Grupo') and session.contains('Listo'))
        with session.sync:
            assert all(name in ''.join(session.cells[0]) for name in ['Archivo','Editar','Buscar','Proyecto','Compilar','Herramientas','Ver','Ayuda'])
            assert sum(ink[3] for ink in session.inks[0])==8
            assert any(fg==(103,212,208) for row in session.inks for fg,bg,bold,under in row)
            assert any(bg==(6,24,34) for row in session.inks for fg,bg,bold,under in row)
        checks.append('spanish_menus_rgb_and_eight_real_alt_underlines')
        stage='menu'
        session.send('\x1ba');session.wait(lambda:session.menu_row('Abrir carpeta'))
        snapshot(session,output/'menu-vt.png');session.send('\x1b');session.wait(lambda:not session.menu_row('Abrir carpeta'))
        session.menu('a','Abrir carpeta');session.wait(lambda:session.contains('Abrir carpeta'))
        session.send(str(output)+'\r');session.wait(lambda:session.contains('F6: cambiar panel') and session.tree_contains('README.md'))
        session.send('\x1b[17~');session.wait(lambda:session.contains('Explorador'))
        snapshot(session,output/'source-vt.png');checks.append('spanish_sidebar_focus_and_visible_navigation_help')
        stage='markdown';session.menu('v','Vista previa de Markdown');session.wait(lambda:session.contains('Markdown · vista previa'))
        session.wait(lambda:session.contains('La vista previa conserva'))
        assert not session.contains('**énfasis**')
        snapshot(session,output/'markdown-vt.png')
        session.send('X\x7f\x1b[3~');session.send('\x1b');session.wait(lambda:not session.contains('Markdown · vista previa'))
        assert not session.contains('1:README.md*');checks.append('markdown_preview_formatted_inert_readonly_and_return_source')
        stage='resize';session.resize(40,12);session.wait(lambda:session.width==40);session.resize(120,32)
        session.wait(lambda:session.contains('F6: cambiar panel'));checks.append('spanish_compact_resize_retains_focus_and_source')
        session.menu('a','Salir');assert session.api.WaitForSingleObject(session.process.process,5000)==0
        code=w.DWORD();session.check(session.api.GetExitCodeProcess(session.process.process,c.byref(code)));assert code.value==0 and source.read_bytes()==original
        report.update(ok=True,checks=checks,exit_code=code.value,fixture_unchanged=True,vt_bytes=session.bytes)
    except BaseException as error:
        report.update(error=str(error),stage=stage,checks=checks,screen=session.text() if session else '');raise
    finally:
        if session:session.close()
        with (output/'report.json').open('x',encoding='utf8') as f:json.dump(report,f,ensure_ascii=False,indent=2)
        print(json.dumps({'output':str(output),**report},ensure_ascii=False))
if __name__=='__main__':main()

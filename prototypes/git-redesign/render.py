"""Propuesta visual, datos ilustrativos; no implementa operaciones Git."""
from pathlib import Path
import re
from PIL import Image, ImageDraw, ImageFont

OUT = Path(__file__).parent
W, H = 1680, 960
C = dict(bg='#2E3440', side='#272D37', panel='#2B313C', bar='#303743',
         header='#353D4A', line='#424B59', fg='#D8DEE9', muted='#929DAF',
         faint='#697586', accent='#88C0D0', selected='#414E62',
         green='#A3BE8C', red='#D58C96', orange='#EBCB8B', purple='#B48EAD',
         add='#30483F', delete='#4A353E')
FONT = 'C:/Windows/Fonts/segoeui.ttf'
BOLD = 'C:/Windows/Fonts/segoeuib.ttf'
MONO = 'C:/Windows/Fonts/consola.ttf'

def render(local=False):
    im = Image.new('RGB', (W, H), C['bg']); d = ImageDraw.Draw(im)
    def box(rect, fill, outline=None): d.rectangle(rect, fill=C.get(fill, fill), outline=C.get(outline, outline))
    def line(points, color='line', width=1): d.line(points, fill=C[color], width=width)
    def text(x, y, s, color='fg', size=13, bold=False, mono=False):
        f = ImageFont.truetype(MONO if mono else BOLD if bold else FONT, size)
        symbols = {'⑂', '⌄', '▥', '⌕', '↻'}
        if any(c in symbols for c in s):
            start=x
            for c in s:
                if c not in symbols:
                    d.text((x,y),c,font=f,fill=C.get(color,color)); x+=d.textlength(c,font=f)
                    continue
                ink=C.get(color,color); cy=y+size/2+3
                if c=='⌄': d.polygon([(x+2,cy-2),(x+8,cy-2),(x+5,cy+2)],fill=ink)
                elif c=='▥':
                    d.rectangle((x+1,cy-5,x+12,cy+5),outline=ink); d.line((x+5,cy-5,x+5,cy+5),fill=ink)
                elif c=='⌕':
                    d.ellipse((x+1,cy-5,x+8,cy+2),outline=ink); d.line((x+8,cy+2,x+12,cy+6),fill=ink)
                elif c=='↻':
                    d.arc((x+1,cy-5,x+12,cy+5),25,315,fill=ink); d.polygon([(x+11,cy-5),(x+13,cy),(x+8,cy-1)],fill=ink)
                else:
                    d.line((x+3,cy+4,x+3,cy-5),fill=ink); d.line((x+3,cy+1,x+9,cy-2,x+9,cy-5),fill=ink)
                    for xx in [x+3,x+9]: d.ellipse((xx-1,cy-7,xx+1,cy-5),fill=ink)
                x+=14
            return x-start
        d.text((x, y), s, font=f, fill=C.get(color, color))
        return d.textlength(s, font=f)
    def pill(x,y,s,color='accent',fill='selected'):
        f=ImageFont.truetype(FONT,11); width=int(d.textlength(s,font=f))+12
        d.rounded_rectangle((x,y,x+width,y+19),radius=3,fill=C[fill]); text(x+6,y+1,s,color,11)
        return width
    def section(x,y,s,n=None):
        text(x,y,'⌄', 'muted',12); text(x+17,y,s,'muted',11,True)
        if n is not None: text(x+150,y,str(n),'faint',11)
    def code(x,y,s):
        # Coloración ilustrativa: texto acotado al espacio de cada columna.
        keys={'DEFINE','VARIABLE','AS','CHARACTER','NO-UNDO','IF','THEN','ELSE','END','ASSIGN','PROCEDURE','INPUT','PARAMETER','DO','RETURN'}
        for tok in re.findall(r'"[^"\n]*"|\b[\w-]+\b|\s+|.',s):
            col='orange' if tok.startswith('"') else 'purple' if tok.upper() in keys else 'accent' if tok in {'cTelefono','ipCliente','obtenerTelefono'} else 'fg'
            x+=text(x,y,tok,col,13,mono=True)
    # Marco CIDE y menús: misma escala que los controles del espacio Git.
    box((0,0,W,31),'side'); box((13,9,26,22),'orange'); text(16,8,'C','side',11,True)
    text(36,6,'CIDE · Corbiz',size=13); text(W-130,6,'—',size=16); text(W-83,5,'□',size=16); text(W-36,5,'×',size=18)
    box((0,32,W,62),'bar')
    x=15
    for s in ['Archivo','Editar','Buscar','Proyecto','Herramientas','Ver','Navegación','Git']:
        x+=text(x,38,s,size=13)+25
    # Pestañas del editor: Git es un espacio dentro de CIDE.
    box((0,63,W,99),'side'); text(16,72,'‹   ›','muted',16)
    text(72,73,'anulaorden.p','muted',12); text(210,72,'×','faint',14)
    line([(231,72),(231,91)]); text(248,73,'anulaorden_crm.p','muted',12); text(415,72,'×','faint',14)
    d.rounded_rectangle((439,65,628,103),radius=5,fill=C['bg'])
    text(457,73,'⑂  Control de versiones',size=12); text(606,72,'×','muted',14)
    text(1625,72,'+   ⌄','muted',15)
    # Barra única y compacta; ninguna herramienta de escritura se simula activa.
    box((0,100,W,142),'bar'); text(17,111,'▥  ⌄','muted',15)
    text(75,111,'‹    ›','muted',15); line([(131,110),(131,132)])
    text(147,112,'Corbiz','fg',13,True); text(208,111,'/','faint',14)
    pill(225,110,'⑂  2026-044  ⌄')
    text(411,113,'Cambios locales' if local else 'Historial', 'muted',12)
    d.rounded_rectangle((1030,107,1435,135),radius=4,fill=C['side'])
    text(1042,112,'⌕   Buscar en el repositorio…','faint',12)
    text(1460,112,'↻  Actualizar','muted',12); text(1584,110,'···','muted',20)
    # Proporciones de referencia: ubicaciones + historial + archivos + diff.
    A,B,D=185,510,740
    box((0,143,A-1,935),'side'); box((A,143,B-1,935),'panel'); box((B,143,D-1,935),'side')
    for x in [A,B,D]: line([(x,143),(x,935)])
    for x,r,s in [(0,A,'UBICACIONES'),(A,B,'HISTORIAL'),(B,D,'ARCHIVOS')]:
        box((x+1,143,r-1,179),'bar'); text(x+14,154,s,'muted',11,True)
    text(A+290,152,'⌕','muted',15); text(B+197,152,'⌕','muted',15)
    section(12,195,'RAMAS',2)
    box((1,219,A-2,247),'selected'); text(27,224,'⑂  2026-044','fg',12); d.ellipse((166,231,171,236),fill=C['green'])
    text(27,256,'⑂  Develop','muted',12)
    section(12,296,'REMOTOS',1); text(28,325,'⌄  origin','muted',12)
    for j,s in enumerate(['2026-044','Develop','CarlosF','Juan','phernandez']): text(46,354+j*27,s,'muted',12)
    section(12,516,'ETIQUETAS',0); section(12,552,'GUARDADOS',0)
    section(12,588,'SUBMÓDULOS',0); section(12,624,'ÁRBOLES DE TRABAJO',1)
    # Cambios locales claramente separados de los commits.
    box((A+1,180,B-1,245),'selected' if local else 'panel')
    d.ellipse((A+16,197,A+23,204),fill=C['orange'])
    text(A+35,187,'Cambios locales',size=13,bold=True); text(A+35,209,'3 modificados · 1 sin seguimiento','muted',11)
    line([(A+12,245),(B-13,245)])
    commits=[('Mejora la consulta de teléfonos','Juan Carlos Gómez · hace 25 min','2026-044'),
             ('Integra Develop en 2026-044','Juan Carlos Gómez · hoy, 10:42','Develop'),
             ('Corrige el cierre de órdenes','Pedro Hernández · ayer, 16:38',''),
             ('Actualiza las reglas de validación','Carlos Fuentes · ayer, 14:20',''),
             ('Ajusta el cálculo de importes','Juan Carlos Gómez · 4 nov',''),
             ('Integra cambios de facturación','Pedro Hernández · 4 nov',''),
             ('Agrega validación de cliente','Carlos Fuentes · 3 nov',''),
             ('Corrige la captura de direcciones','Juan Carlos Gómez · 3 nov',''),
             ('Actualiza documentación','Pedro Hernández · 2 nov','')]
    # Trazo primer padre; no se presenta un DAG inventado como función existente.
    line([(A+20,264),(A+20,877)],'accent',2)
    for j,(title,who,tag) in enumerate(commits):
        y=254+j*73
        if j==0 and not local: box((A+1,y,B-2,y+72),'selected')
        d.ellipse((A+16,y+13,A+24,y+21),fill=C['accent'],outline=C['bg'],width=1)
        text(A+35,y+8,title,size=12)
        text(A+35,y+30,who,'muted',11)
        if tag: pill(A+35,y+49,tag,'accent' if j==0 else 'muted')
    box((B-7,265,B-4,539),'line')
    section(B+12,195,'SIN PREPARAR' if local else 'CAMBIOS',3)
    paths=['SolicitudEtiquetas_Andesmar.p','cierraorden.p','imp_facmextic88v.p']
    for j,p in enumerate(paths):
        y=223+j*57
        if j==0: box((B+1,y-3,D-2,y+49),'selected')
        text(B+13,y,'M','orange',11,True); text(B+34,y,p,size=11)
        text(B+34,y+20,'emb-centros / procesos' if j==0 else 'ventas / punto','muted',10)
    if local:
        section(B+12,425,'SIN SEGUIMIENTO',1)
        text(B+14,454,'?','accent',12); text(B+34,454,'notas-validacion.md',size=11)
        box((B+12,826,D-12,911),'panel'); text(B+23,840,'Revisión de cambios','muted',12,True)
        text(B+23,866,'Índice ↔ archivo en disco','muted',11)
        text(B+23,886,'Consulta de solo lectura','faint',11)
    # Pestañas contenidas en el panel de detalle.
    box((D+1,143,W,179),'bar'); text(D+16,154,'Resumen','muted',12)
    box((D+99,144,D+382,178),'bg'); text(D+112,154,paths[0],size=12)
    text(D+365,152,'×','muted',14); text(W-40,151,'···','muted',18)
    if local:
        text(D+22,195,'Cambios sin preparar',size=18,bold=True)
        text(D+22,227,'SolicitudEtiquetas_Andesmar.p','muted',12)
        pill(D+22,255,'Índice'); text(D+78,256,'→','muted',14); pill(D+103,255,'Disco')
        text(D+180,257,'Archivo modificado · +11  −3','muted',12)
    else:
        text(D+22,194,'Mejora la consulta de teléfonos',size=18,bold=True)
        text(D+22,227,'Juan Carlos Gómez','fg',12); text(D+155,227,'·  hace 25 min  ·  6 nov 2026, 11:20','muted',12)
        pill(D+22,255,'2026-044'); text(D+110,257,'25a66c11','accent',12,mono=True)
        text(D+198,257,'3 archivos modificados', 'muted',12)
        text(W-132,257,'−3','red',12); text(W-91,257,'+11','green',12)
    # Comparación protagonista: cabecera y controles discretos.
    y0=296
    box((D+1,y0,W-1,y0+35),'header')
    text(D+15,y0+10,'⌄  emb-centros / procesos / '+paths[0],size=12)
    text(W-234,y0+10,'Unificada  ⌄','fg',12); text(W-111,y0+8,'↑   ↓','muted',15); text(W-35,y0+6,'···','muted',18)
    text(D+21,y0+46,'@@ 188,10  →  188,18  ·  inicializar:','muted',11,mono=True)
    rows=[('ctx',188,188,'    WHERE orden_v_exten.NO_orden = ipNoOrden'),
          ('ctx',189,189,'      AND orden_v_exten.cent_dist = ipCentDist'),
          ('ctx',190,190,'    NO-LOCK:'),
          ('del',191,None,'    IF orden_v_exten.udf_c[1] <> "" THEN'),
          ('del',192,None,'        ASSIGN cTelefono = orden_v_exten.udf_c[1].'),
          ('add',None,191,'    /* Prioriza el teléfono de contacto. */'),
          ('add',None,192,'    IF orden_v_exten.udf_c[1] <> "" THEN DO:'),
          ('add',None,193,'        cTelefono = orden_v_exten.udf_c[1].'),
          ('add',None,194,'    END.'),
          ('add',None,195,'    ELSE IF orden_v_exten.udf_c[2] <> "" THEN DO:'),
          ('add',None,196,'        cTelefono = orden_v_exten.udf_c[2].'),
          ('add',None,197,'    END.'),
          ('add',None,198,'    ELSE DO:'),
          ('add',None,199,'        cTelefono = "No tiene".'),
          ('add',None,200,'    END.'),
          ('ctx',193,201,'END.'),
          ('ctx',194,202,''),
          ('ctx',195,203,'/* BÚSQUEDA DEL CLIENTE */'),
          ('ctx',196,204,'FOR FIRST cliente NO-LOCK'),
          ('ctx',197,205,'    WHERE cliente.id = ipCliente:')]
    yy=y0+74
    for kind,a,b,s in rows:
        if kind!='ctx': box((D+1,yy,W-13,yy+23),'add' if kind=='add' else 'delete')
        box((D+1,yy,D+89,yy+23),'panel' if kind=='ctx' else 'add' if kind=='add' else 'delete')
        text(D+12,yy+3,str(a or ''),'faint',12,mono=True); text(D+52,yy+3,str(b or ''),'muted',12,mono=True)
        text(D+97,yy+3,'+' if kind=='add' else '−' if kind=='del' else '','green' if kind=='add' else 'red',12,mono=True)
        if '/*' in s: text(D+118,yy+3,s,'green',13,mono=True)
        else: code(D+118,yy+3,s)
        yy+=24
    box((W-10,342,W-5,590),'line'); box((D+8,912,W-19,918),'side'); box((D+8,912,W-168,918),'line')
    # Estado compacto, sin datos del motor ni instrucciones extensas.
    box((0,936,W,H),'side'); line([(0,935),(W,935)])
    text(13,941,'▥', 'muted',12); text(42,941,'⑂ 2026-044','accent',11)
    text(163,941,'4 cambios locales','muted',11)
    text(D+15,941,'Índice ↔ disco' if local else 'Comparación con el primer padre','muted',11)
    text(W-255,941,'Solo lectura', 'muted',11); text(W-148,941,'ABL', 'accent',11); text(W-95,941,'UTF-8', 'muted',11)
    path=OUT/('cide-git-cambios.png' if local else 'cide-git-historial.png')
    im.save(path)
    return path

if __name__=='__main__':
    for mode in [False,True]: print(render(mode))

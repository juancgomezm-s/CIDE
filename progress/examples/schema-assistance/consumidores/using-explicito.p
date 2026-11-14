USING alterno.* FROM PROPATH.
USING paquete.Elemento FROM PROPATH.
DEFINE VARIABLE objeto AS Elemento NO-UNDO.
objeto = NEW Elemento().
objeto = CAST(objeto, Elemento).
DISPLAY objeto:Primero().

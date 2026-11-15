USING paquete.Derivada FROM PROPATH.
DEFINE VARIABLE objeto AS CLASS Derivada NO-UNDO.
DEFINE VARIABLE numero AS INTEGER NO-UNDO INITIAL 7.
objeto = NEW Derivada().
objeto:Titulo = objeto:Saludar("Ana").
DISPLAY objeto:Saludar(12) objeto:Saludar(numero) objeto:Titulo.

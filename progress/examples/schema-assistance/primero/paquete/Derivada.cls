USING paquete.Intermedia FROM PROPATH.
CLASS paquete.Derivada INHERITS Intermedia:
    METHOD PROTECTED OVERRIDE CHARACTER Saludo():
        RETURN SUPER:Saludo().
    END METHOD.

    METHOD PUBLIC CHARACTER Probar():
        DEFINE VARIABLE texto AS CHARACTER NO-UNDO.
        texto = SUPER:Saludar("Ana").
        THIS-OBJECT:Titulo = texto.
        RETURN THIS-OBJECT:Saludo().
    END METHOD.
END CLASS.

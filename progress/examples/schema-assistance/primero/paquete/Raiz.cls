CLASS paquete.Raiz:
    DEFINE PUBLIC PROPERTY Titulo AS CHARACTER NO-UNDO GET. SET.
    DEFINE PUBLIC PROPERTY Hijo AS CLASS paquete.Raiz NO-UNDO GET. SET.

    METHOD PUBLIC paquete.Raiz Obtener():
        RETURN THIS-OBJECT.
    END METHOD.

    METHOD PUBLIC VOID Ajustar(INPUT cantidad AS INTEGER):
    END METHOD.

    METHOD PUBLIC VOID Ajustar(OUTPUT cantidad AS INTEGER):
        cantidad = 21.
    END METHOD.

    METHOD PUBLIC VOID Ajustar(INPUT-OUTPUT cantidad AS INTEGER):
        cantidad = cantidad + 1.
    END METHOD.

    METHOD PUBLIC CHARACTER Saludar(INPUT nombre AS CHARACTER):
        RETURN nombre.
    END METHOD.

    METHOD PUBLIC CHARACTER Saludar(INPUT cantidad AS INTEGER):
        RETURN STRING(cantidad).
    END METHOD.

    METHOD PROTECTED CHARACTER Saludo():
        RETURN "raiz".
    END METHOD.
END CLASS.

{declaraciones.i}
FOR EACH bCliente NO-LOCK:
    DISPLAY bCliente.name.
END.
FOR EACH bTrabajo USE-INDEX porCodigo:
    DISPLAY bTrabajo.Codigo.
END.
FOR EACH ttCopia USE-INDEX cust-num:
    DISPLAY ttCopia.name ttCopia.extra.
END.
DISPLAY vIncluido fIncluida().
RUN pIncluido.

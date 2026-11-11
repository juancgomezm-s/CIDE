/* JC-IDE schema reader. Original implementation.
   Called only by jc-bridge.p after a connected DB receives the DICTDB alias.
   Output inherits the caller's ASCII protocol stream. No database writes. */

DEFINE INPUT PARAMETER pcDatabase AS CHARACTER NO-UNDO.
DEFINE OUTPUT PARAMETER plEncodingFailed AS LOGICAL NO-UNDO.

FUNCTION HexText RETURNS CHARACTER (INPUT pcText AS CHARACTER):
    DEFINE VARIABLE mText  AS MEMPTR    NO-UNDO.
    DEFINE VARIABLE cHex   AS CHARACTER NO-UNDO INITIAL "".
    DEFINE VARIABLE iByte AS INTEGER   NO-UNDO.
    DEFINE VARIABLE iLength AS INTEGER NO-UNDO.
    DEFINE VARIABLE i     AS INTEGER   NO-UNDO.
    IF pcText = ? THEN RETURN "-".
    iLength = LENGTH(pcText, "RAW").
    IF iLength > 8192 THEN DO:
        plEncodingFailed = TRUE.
        RETURN "".
    END.
    SET-SIZE(mText) = iLength + 1.
    PUT-STRING(mText, 1) = pcText.
    DO i = 1 TO iLength:
        iByte = GET-BYTE(mText, i).
        cHex = cHex
             + SUBSTRING("0123456789ABCDEF", INTEGER(TRUNCATE(iByte / 16, 0)) + 1, 1)
             + SUBSTRING("0123456789ABCDEF", (iByte MODULO 16) + 1, 1).
    END.
    SET-SIZE(mText) = 0.
    RETURN cHex.
END FUNCTION.

plEncodingFailed = FALSE.

FOR EACH DICTDB._File NO-LOCK
    WHERE DICTDB._File._Hidden = FALSE
      AND DICTDB._File._File-Number > 0
    BY DICTDB._File._File-Name:
    PUT UNFORMATTED "TABLE|" HexText(pcDatabase) "|"
        HexText(DICTDB._File._File-Name) SKIP.

    FOR EACH DICTDB._Field OF DICTDB._File NO-LOCK
        BY DICTDB._Field._Order:
        PUT UNFORMATTED "FIELD|" HexText(pcDatabase) "|"
            HexText(DICTDB._File._File-Name) "|"
            HexText(DICTDB._Field._Field-Name) "|"
            HexText(DICTDB._Field._Data-Type) "|"
            TRIM(STRING(DICTDB._Field._Extent, ">>>>>>>>>9")) "|"
            HexText(DICTDB._Field._Format) "|"
            HexText(DICTDB._Field._Label) SKIP.
    END.

    FOR EACH DICTDB._Index OF DICTDB._File NO-LOCK
        BY DICTDB._Index._Index-Name:
        PUT UNFORMATTED "INDEX|" HexText(pcDatabase) "|"
            HexText(DICTDB._File._File-Name) "|"
            HexText(DICTDB._Index._Index-Name) "|"
            (IF RECID(DICTDB._Index) = DICTDB._File._Prime-Index THEN "1" ELSE "0") "|"
            (IF DICTDB._Index._Unique THEN "1" ELSE "0") SKIP.

        FOR EACH DICTDB._Index-Field OF DICTDB._Index NO-LOCK
            BY DICTDB._Index-Field._Index-Seq:
            FIND DICTDB._Field OF DICTDB._Index-Field NO-LOCK.
            PUT UNFORMATTED "INDEX_FIELD|" HexText(pcDatabase) "|"
                HexText(DICTDB._File._File-Name) "|"
                HexText(DICTDB._Index._Index-Name) "|"
                TRIM(STRING(DICTDB._Index-Field._Index-Seq, ">>>>>>>>>9")) "|"
                HexText(DICTDB._Field._Field-Name) "|"
                (IF DICTDB._Index-Field._Ascending THEN "1" ELSE "0") SKIP.
        END.
    END.
END.

/* JC-IDE experimental ABL bridge. Original implementation.
   The editor starts a NEW batch AVM with project .pf/.ini and explicit
   -cpstream/-cpinternal. -param contains ONE absolute ASCII request path.
   See docs/PROGRESS_BRIDGE.md.
   Source bytes are read by the installed compiler, never transcoded here. */

DEFINE STREAM requestInput.
DEFINE VARIABLE cHeader       AS CHARACTER NO-UNDO.
DEFINE VARIABLE cAction       AS CHARACTER NO-UNDO.
DEFINE VARIABLE cSource       AS CHARACTER NO-UNDO.
DEFINE VARIABLE cOutputDir    AS CHARACTER NO-UNDO.
DEFINE VARIABLE cProjectPath  AS CHARACTER NO-UNDO.
DEFINE VARIABLE cResponse     AS CHARACTER NO-UNDO.
DEFINE VARIABLE cSelf         AS CHARACTER NO-UNDO.
DEFINE VARIABLE cHelperDir    AS CHARACTER NO-UNDO.
DEFINE VARIABLE cDatabase     AS CHARACTER NO-UNDO.
DEFINE VARIABLE cErrorFile    AS CHARACTER NO-UNDO.
DEFINE VARIABLE cSeverity     AS CHARACTER NO-UNDO.
DEFINE VARIABLE cProgram      AS CHARACTER NO-UNDO.
DEFINE VARIABLE cAliases      AS CHARACTER NO-UNDO.
DEFINE VARIABLE cAlias        AS CHARACTER NO-UNDO.
DEFINE VARIABLE cAliasPair    AS CHARACTER NO-UNDO.
DEFINE VARIABLE cExpectedVersion AS CHARACTER NO-UNDO.
DEFINE VARIABLE cAvmVersion   AS CHARACTER NO-UNDO.
DEFINE VARIABLE iAlias        AS INTEGER   NO-UNDO.
DEFINE VARIABLE iSlash        AS INTEGER   NO-UNDO.
DEFINE VARIABLE iMessage      AS INTEGER   NO-UNDO.
DEFINE VARIABLE iMessages     AS INTEGER   NO-UNDO.
DEFINE VARIABLE iDatabase     AS INTEGER   NO-UNDO.
DEFINE VARIABLE iRow          AS INTEGER   NO-UNDO.
DEFINE VARIABLE iColumn       AS INTEGER   NO-UNDO.
DEFINE VARIABLE lRead         AS LOGICAL   NO-UNDO INITIAL FALSE.
DEFINE VARIABLE lFailed       AS LOGICAL   NO-UNDO INITIAL FALSE.
DEFINE VARIABLE lEncodingFailed AS LOGICAL NO-UNDO INITIAL FALSE.
DEFINE VARIABLE lSchemaEncodingFailed AS LOGICAL NO-UNDO INITIAL FALSE.

FUNCTION HexText RETURNS CHARACTER (INPUT pcText AS CHARACTER):
    DEFINE VARIABLE mText  AS MEMPTR    NO-UNDO.
    DEFINE VARIABLE cHex   AS CHARACTER NO-UNDO INITIAL "".
    DEFINE VARIABLE iByte AS INTEGER   NO-UNDO.
    DEFINE VARIABLE iLength AS INTEGER NO-UNDO.
    DEFINE VARIABLE i     AS INTEGER   NO-UNDO.
    IF pcText = ? THEN RETURN "-".
    iLength = LENGTH(pcText, "RAW").
    IF iLength > 8192 THEN DO:
        lEncodingFailed = TRUE.
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

FUNCTION PositionText RETURNS CHARACTER (INPUT piValue AS INTEGER):
    IF piValue = ? OR piValue <= 0 THEN RETURN "-".
    RETURN TRIM(STRING(piValue, ">>>>>>>>>9")).
END FUNCTION.

DO ON ERROR UNDO, LEAVE ON ENDKEY UNDO, LEAVE:
    INPUT STREAM requestInput FROM VALUE(SESSION:PARAMETER) NO-CONVERT.
    IMPORT STREAM requestInput UNFORMATTED cHeader.
    IMPORT STREAM requestInput UNFORMATTED cAction.
    IMPORT STREAM requestInput UNFORMATTED cSource.
    IMPORT STREAM requestInput UNFORMATTED cOutputDir.
    IMPORT STREAM requestInput UNFORMATTED cProjectPath.
    IF cHeader = "JCIDE/2" OR cHeader = "JCIDE/3" THEN
        IMPORT STREAM requestInput UNFORMATTED cAliases.
    IF cHeader = "JCIDE/3" THEN
        IMPORT STREAM requestInput UNFORMATTED cExpectedVersion.
    INPUT STREAM requestInput CLOSE.
    lRead = TRUE.
END.
IF NOT lRead OR (cHeader <> "JCIDE/1" AND cHeader <> "JCIDE/2" AND cHeader <> "JCIDE/3")
    OR cOutputDir = "" THEN QUIT.

cResponse = cOutputDir + "/response.txt".
FILE-INFO:FILE-NAME = cResponse.
IF FILE-INFO:FULL-PATHNAME <> ? THEN QUIT.

cSelf = THIS-PROCEDURE:FILE-NAME.
iSlash = MAXIMUM(R-INDEX(cSelf, "/"), R-INDEX(cSelf, CHR(92))).
cHelperDir = SUBSTRING(cSelf, 1, iSlash).
IF cProjectPath <> "" THEN PROPATH = cProjectPath.

OUTPUT TO VALUE(cResponse) NO-CONVERT.
IF cHeader = "JCIDE/3" THEN DO:
    cAvmVersion = PROVERSION.
    PUT UNFORMATTED "JCIDE|2|" HexText(SESSION:CPINTERNAL)
        "|" HexText(SESSION:CPSTREAM) "|" HexText(cAvmVersion) SKIP.
    IF cExpectedVersion <> "" AND cExpectedVersion <> cAvmVersion THEN DO:
        PUT UNFORMATTED "DIAG|error|-|-|-|"
            HexText("Version del AVM: " + cAvmVersion + ". El perfil requiere " + cExpectedVersion + ".") SKIP.
        PUT UNFORMATTED "END|ERROR" SKIP.
        OUTPUT CLOSE.
        QUIT.
    END.
END.
ELSE PUT UNFORMATTED "JCIDE|1|" HexText(SESSION:CPINTERNAL)
    "|" HexText(SESSION:CPSTREAM) SKIP.

DO iAlias = 1 TO NUM-ENTRIES(cAliases):
    cAliasPair = ENTRY(iAlias, cAliases).
    IF NUM-ENTRIES(cAliasPair, "=") <> 2 THEN DO:
        lFailed = TRUE.
        PUT UNFORMATTED "DIAG|error|-|-|-|" HexText("Alias ABL invalido.") SKIP.
        LEAVE.
    END.
    ASSIGN cAlias = ENTRY(1, cAliasPair, "=")
           cDatabase = ENTRY(2, cAliasPair, "=").
    IF cAlias = "DICTDB" OR cAlias = "" OR cDatabase = ""
        OR LDBNAME(cDatabase) = ? OR LDBNAME(cDatabase) <> cDatabase THEN DO:
        lFailed = TRUE.
        PUT UNFORMATTED "DIAG|error|-|-|-|"
            HexText("Alias " + cAlias + ": base logica no disponible o alias reservado: " + cDatabase) SKIP.
        LEAVE.
    END.
    CREATE ALIAS VALUE(cAlias) FOR DATABASE VALUE(cDatabase) NO-ERROR.
    IF ERROR-STATUS:ERROR OR LDBNAME(cAlias) = ? OR LDBNAME(cAlias) <> cDatabase THEN DO:
        lFailed = TRUE.
        PUT UNFORMATTED "DIAG|error|-|-|-|"
            HexText("No se pudo crear el alias " + cAlias + " para " + cDatabase + ".") SKIP.
        LEAVE.
    END.
END.

IF NOT lFailed THEN CASE cAction:
    WHEN "check" OR WHEN "compile" OR WHEN "run" OR WHEN "xref" THEN DO:
        IF cSource = "" THEN DO:
            lFailed = TRUE.
            PUT UNFORMATTED "DIAG|error|-|-|-|" HexText("Source path is required.") SKIP.
        END.
        ELSE DO:
            IF cAction = "xref" THEN
                COMPILE VALUE(cSource) XREF VALUE(cOutputDir + "/references.xrf") NO-ERROR.
            ELSE
                COMPILE VALUE(cSource) SAVE INTO VALUE(cOutputDir) NO-ERROR.
            ASSIGN lFailed    = COMPILER:ERROR
                   iMessages  = ERROR-STATUS:NUM-MESSAGES
                   cErrorFile = COMPILER:FILE-NAME
                   iRow       = COMPILER:ERROR-ROW
                   iColumn    = COMPILER:ERROR-COLUMN
                   cSeverity  = IF lFailed THEN "error" ELSE "warning".
            DO iMessage = 1 TO iMessages:
                PUT UNFORMATTED "DIAG|" cSeverity "|"
                    (IF iMessage = 1 AND lFailed THEN PositionText(iRow) ELSE "-") "|"
                    (IF iMessage = 1 AND lFailed THEN PositionText(iColumn) ELSE "-") "|"
                    (IF iMessage = 1 AND lFailed THEN HexText(cErrorFile) ELSE "-") "|"
                    HexText(ERROR-STATUS:GET-MESSAGE(iMessage)) SKIP.
            END.
            IF lFailed AND iMessages = 0 THEN
                PUT UNFORMATTED "DIAG|error|-|-|-|"
                    HexText("Compilation failed without a diagnostic message.") SKIP.
            IF cAction = "run" AND NOT lFailed THEN DO:
                iSlash = MAXIMUM(R-INDEX(cSource, "/"), R-INDEX(cSource, CHR(92))).
                cProgram = SUBSTRING(cSource, iSlash + 1).
                cProgram = cOutputDir + "/" + SUBSTRING(cProgram, 1, R-INDEX(cProgram, ".") - 1) + ".r".
                OUTPUT CLOSE.
                RUN VALUE(cProgram) NO-ERROR.
                ASSIGN lFailed = ERROR-STATUS:ERROR OR ERROR-STATUS:NUM-MESSAGES > 0
                       iMessages = ERROR-STATUS:NUM-MESSAGES.
                OUTPUT TO VALUE(cResponse) APPEND NO-CONVERT.
                DO iMessage = 1 TO iMessages:
                    PUT UNFORMATTED "DIAG|error|-|-|-|"
                        HexText(ERROR-STATUS:GET-MESSAGE(iMessage)) SKIP.
                END.
                IF lFailed AND iMessages = 0 THEN
                    PUT UNFORMATTED "DIAG|error|-|-|-|"
                        HexText(IF RETURN-VALUE = "" OR RETURN-VALUE = ? THEN
                            "Execution failed." ELSE RETURN-VALUE) SKIP.
            END.
        END.
    END.
    WHEN "probe" THEN DO:
        IF NUM-DBS = 0 THEN DO:
            lFailed = TRUE.
            PUT UNFORMATTED "DIAG|error|-|-|-|"
                HexText("El AVM inicio, pero no hay bases de datos conectadas.") SKIP.
        END.
        ELSE DO iDatabase = 1 TO NUM-DBS:
            PUT UNFORMATTED "DATABASE|" HexText(LDBNAME(iDatabase)) SKIP.
        END.
    END.
    WHEN "schema" THEN DO:
        IF cSource <> "" OR NUM-DBS = 0 THEN DO:
            lFailed = TRUE.
            PUT UNFORMATTED "DIAG|error|-|-|-|"
                    HexText("Se requieren bases conectadas y una solicitud sin fuente.") SKIP.
        END.
        ELSE DO iDatabase = 1 TO NUM-DBS:
            cDatabase = LDBNAME(iDatabase).
            IF cDatabase <> "DICTDB" THEN DO:
                DELETE ALIAS "DICTDB".
                CREATE ALIAS "DICTDB" FOR DATABASE VALUE(cDatabase) NO-ERROR.
            END.
            IF LDBNAME("DICTDB") = ? OR LDBNAME("DICTDB") <> cDatabase THEN DO:
                lFailed = TRUE.
                PUT UNFORMATTED "DIAG|error|-|-|-|"
                    HexText("No se pudo vincular DICTDB a " + cDatabase + ".") SKIP.
                NEXT.
            END.
            IF cHeader = "JCIDE/2" OR cHeader = "JCIDE/3" THEN
                PUT UNFORMATTED "DATABASE|" HexText(cDatabase) SKIP.
            lSchemaEncodingFailed = FALSE.
            RUN VALUE(cHelperDir + "jc-schema.p")
                (INPUT cDatabase, OUTPUT lSchemaEncodingFailed) NO-ERROR.
            IF lSchemaEncodingFailed THEN lEncodingFailed = TRUE.
            IF ERROR-STATUS:ERROR OR ERROR-STATUS:NUM-MESSAGES > 0 THEN DO:
                lFailed = TRUE.
                DO iMessage = 1 TO ERROR-STATUS:NUM-MESSAGES:
                    PUT UNFORMATTED "DIAG|error|-|-|-|"
                        HexText(ERROR-STATUS:GET-MESSAGE(iMessage)) SKIP.
                END.
            END.
        END.
    END.
    OTHERWISE DO:
        lFailed = TRUE.
        PUT UNFORMATTED "DIAG|error|-|-|-|"
            HexText("Accion ABL desconocida.") SKIP.
    END.
END CASE.

IF lEncodingFailed THEN DO:
    lFailed = TRUE.
    PUT UNFORMATTED "DIAG|error|-|-|-|"
        HexText("A response field exceeds the 8192-byte ABL emitter limit. This partial response must not replace the schema cache.") SKIP.
END.

PUT UNFORMATTED "END|" (IF lFailed THEN "ERROR" ELSE "OK") SKIP.
OUTPUT CLOSE.
QUIT.

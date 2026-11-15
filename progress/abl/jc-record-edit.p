/* Single-row local-copy editor. The Rust host publishes the backup first. */
DEFINE TEMP-TABLE ttImage NO-UNDO FIELD payload AS RAW.
DEFINE STREAM inputFile.
DEFINE STREAM outputFile.
DEFINE VARIABLE cHeader AS CHARACTER NO-UNDO.
DEFINE VARIABLE cMode AS CHARACTER NO-UNDO.
DEFINE VARIABLE cVersion AS CHARACTER NO-UNDO.
DEFINE VARIABLE cDb AS CHARACTER NO-UNDO.
DEFINE VARIABLE cTable AS CHARACTER NO-UNDO.
DEFINE VARIABLE cIndex AS CHARACTER NO-UNDO.
DEFINE VARIABLE cKeyField AS CHARACTER NO-UNDO.
DEFINE VARIABLE cKey AS CHARACTER NO-UNDO.
DEFINE VARIABLE cColumn AS CHARACTER NO-UNDO.
DEFINE VARIABLE cOldRow AS CHARACTER NO-UNDO.
DEFINE VARIABLE cOldImage AS CHARACTER NO-UNDO.
DEFINE VARIABLE cNext AS CHARACTER NO-UNDO.
DEFINE VARIABLE cValue AS CHARACTER CASE-SENSITIVE NO-UNDO.
DEFINE VARIABLE cAfter AS CHARACTER CASE-SENSITIVE NO-UNDO.
DEFINE VARIABLE cImage AS CHARACTER CASE-SENSITIVE NO-UNDO.
DEFINE VARIABLE cRow AS CHARACTER NO-UNDO.
DEFINE VARIABLE cInfo AS CHARACTER NO-UNDO.
DEFINE VARIABLE cPredicate AS CHARACTER NO-UNDO.
DEFINE VARIABLE cStatus AS CHARACTER NO-UNDO INITIAL "write".
DEFINE VARIABLE iKey AS INTEGER NO-UNDO.
DEFINE VARIABLE iNext AS INTEGER NO-UNDO.
DEFINE VARIABLE i AS INTEGER NO-UNDO.
DEFINE VARIABLE hBuffer AS HANDLE NO-UNDO.
DEFINE VARIABLE hEdit AS HANDLE NO-UNDO.
DEFINE VARIABLE hRaw AS HANDLE NO-UNDO.
DEFINE VARIABLE hField AS HANDLE NO-UNDO.
DEFINE VARIABLE lOk AS LOGICAL NO-UNDO.
DEFINE VARIABLE lRead AS LOGICAL NO-UNDO.
DEFINE VARIABLE lCommitted AS LOGICAL INITIAL FALSE.

FUNCTION SafeName RETURNS LOGICAL (INPUT pcName AS CHARACTER):
    DEFINE VARIABLE idx AS INTEGER NO-UNDO.
    DEFINE VARIABLE alphabet AS CHARACTER NO-UNDO INITIAL "ABCDEFGHIJKLMNOPQRSTUVWXYZ".
    IF pcName = ? OR LENGTH(pcName) = 0 OR LENGTH(pcName) > 64 THEN RETURN FALSE.
    IF INDEX(alphabet, CAPS(SUBSTRING(pcName, 1, 1))) = 0 THEN RETURN FALSE.
    DO idx = 1 TO LENGTH(pcName):
        IF INDEX(alphabet + "0123456789_-", CAPS(SUBSTRING(pcName, idx, 1))) = 0 THEN RETURN FALSE.
    END.
    RETURN TRUE.
END FUNCTION.

FUNCTION RawHex RETURNS CHARACTER (INPUT dataBytes AS RAW):
    DEFINE VARIABLE idx AS INTEGER NO-UNDO.
    DEFINE VARIABLE b AS INTEGER NO-UNDO.
    DEFINE VARIABLE cResult AS CHARACTER NO-UNDO INITIAL "".
    DEFINE VARIABLE digits AS CHARACTER NO-UNDO INITIAL "0123456789ABCDEF".
    IF dataBytes = ? OR LENGTH(dataBytes) = 0 OR LENGTH(dataBytes) > 8192 THEN RETURN ?.
    DO idx = 1 TO LENGTH(dataBytes):
        b = GET-BYTE(dataBytes, idx).
        cResult = cResult + SUBSTRING(digits, INTEGER(TRUNCATE(b / 16, 0)) + 1, 1)
                         + SUBSTRING(digits, b MODULO 16 + 1, 1).
    END.
    RETURN cResult.
END FUNCTION.

FUNCTION TextHex RETURNS CHARACTER (INPUT pcText AS CHARACTER):
    DEFINE VARIABLE dataBytes AS RAW NO-UNDO.
    DEFINE VARIABLE iLength AS INTEGER NO-UNDO.
    IF pcText = ? THEN RETURN "-".
    iLength = LENGTH(pcText, "RAW").
    IF iLength = 0 THEN RETURN "".
    IF iLength > 1024 THEN RETURN ?.
    LENGTH(dataBytes) = iLength.
    PUT-STRING(dataBytes, 1, iLength) = pcText.
    RETURN RawHex(dataBytes).
END FUNCTION.

FUNCTION TextFromHex RETURNS CHARACTER (INPUT pcHex AS CHARACTER):
    DEFINE VARIABLE dataBytes AS MEMPTR NO-UNDO.
    DEFINE VARIABLE idx AS INTEGER NO-UNDO.
    DEFINE VARIABLE hi AS INTEGER NO-UNDO.
    DEFINE VARIABLE lo AS INTEGER NO-UNDO.
    DEFINE VARIABLE digits AS CHARACTER NO-UNDO INITIAL "0123456789ABCDEF".
    DEFINE VARIABLE cResult AS CHARACTER NO-UNDO.
    IF pcHex = "-" THEN RETURN ?.
    IF pcHex = "" THEN RETURN "".
    IF LENGTH(pcHex) > 2048 OR LENGTH(pcHex) MODULO 2 <> 0 THEN RETURN ?.
    SET-SIZE(dataBytes) = INTEGER(LENGTH(pcHex) / 2) + 1.
    PUT-BYTE(dataBytes, GET-SIZE(dataBytes)) = 0.
    DO idx = 1 TO LENGTH(pcHex) BY 2:
        ASSIGN hi = INDEX(digits, SUBSTRING(pcHex, idx, 1))
               lo = INDEX(digits, SUBSTRING(pcHex, idx + 1, 1)).
        IF hi = 0 OR lo = 0 OR (hi = 1 AND lo = 1) THEN DO:
            SET-SIZE(dataBytes) = 0.
            RETURN ?.
        END.
        PUT-BYTE(dataBytes, INTEGER((idx + 1) / 2)) = (hi - 1) * 16 + lo - 1.
    END.
    cResult = GET-STRING(dataBytes, 1).
    SET-SIZE(dataBytes) = 0.
    RETURN cResult.
END FUNCTION.

FUNCTION FieldValue RETURNS CHARACTER (INPUT fieldHandle AS HANDLE):
    IF fieldHandle:BUFFER-VALUE = ? THEN RETURN "-".
    IF fieldHandle:DATA-TYPE = "INTEGER" THEN RETURN TextHex(TRIM(STRING(INTEGER(fieldHandle:BUFFER-VALUE)))).
    RETURN TextHex(STRING(fieldHandle:BUFFER-VALUE)).
END FUNCTION.

DO ON ERROR UNDO, LEAVE ON ENDKEY UNDO, LEAVE:
    INPUT STREAM inputFile FROM VALUE(SESSION:PARAMETER) NO-CONVERT.
    IMPORT STREAM inputFile UNFORMATTED cHeader.
    IMPORT STREAM inputFile UNFORMATTED cMode.
    IMPORT STREAM inputFile UNFORMATTED cVersion.
    IMPORT STREAM inputFile UNFORMATTED cDb.
    IMPORT STREAM inputFile UNFORMATTED cTable.
    IMPORT STREAM inputFile UNFORMATTED cIndex.
    IMPORT STREAM inputFile UNFORMATTED cKeyField.
    IMPORT STREAM inputFile UNFORMATTED cKey.
    IMPORT STREAM inputFile UNFORMATTED cColumn.
    IMPORT STREAM inputFile UNFORMATTED cOldRow.
    IMPORT STREAM inputFile UNFORMATTED cOldImage.
    IMPORT STREAM inputFile UNFORMATTED cNext.
    INPUT STREAM inputFile CLOSE.
    lRead = TRUE.
END.
IF NOT lRead OR cHeader <> "JCIDE-RECORD/1" THEN QUIT.
OUTPUT STREAM outputFile TO VALUE(SUBSTRING(SESSION:PARAMETER, 1, R-INDEX(SESSION:PARAMETER, ".")) + "out") NO-CONVERT.
PUT STREAM outputFile UNFORMATTED cHeader SKIP "AVM|" PROVERSION SKIP.
IF cMode <> "preview" AND cMode <> "apply" THEN RUN Fail("shape").
IF cVersion <> PROVERSION THEN RUN Fail("version").
IF NUM-DBS <> 1 OR LDBNAME(1) <> cDb THEN RUN Fail("connection").
IF (INDEX(DBRESTRICTIONS(1), "READ-ONLY") > 0) <> (cMode = "preview") THEN RUN Fail("connection").
IF NOT SafeName(cDb) OR NOT SafeName(cTable) OR NOT SafeName(cIndex) OR NOT SafeName(cKeyField)
   OR NOT SafeName(cColumn) OR cColumn = cKeyField THEN RUN Fail("shape").
iKey = INTEGER(cKey) NO-ERROR.
IF ERROR-STATUS:ERROR OR iKey = ? OR TRIM(STRING(iKey)) <> cKey THEN RUN Fail("shape").

CREATE BUFFER hBuffer FOR TABLE (cDb + "." + cTable) BUFFER-NAME "cide_edit" NO-ERROR.
IF NOT VALID-HANDLE(hBuffer) THEN RUN Fail("shape").
lOk = hBuffer:DISABLE-DUMP-TRIGGERS() NO-ERROR.
IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN RUN Fail("triggers").
IF cMode = "apply" THEN DO:
    lOk = hBuffer:DISABLE-LOAD-TRIGGERS(FALSE) NO-ERROR.
    IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN RUN Fail("triggers").
END.
DO i = 1 TO hBuffer:NUM-FIELDS:
    hField = hBuffer:BUFFER-FIELD(i).
    IF LOOKUP(hField:DATA-TYPE, "BLOB,CLOB") > 0 THEN RUN Fail("shape").
END.
hField = hBuffer:BUFFER-FIELD(cKeyField) NO-ERROR.
IF NOT VALID-HANDLE(hField) OR hField:NAME <> cKeyField OR hField:DATA-TYPE <> "INTEGER"
   OR hField:EXTENT <> 0 THEN RUN Fail("shape").
hEdit = hBuffer:BUFFER-FIELD(cColumn) NO-ERROR.
IF NOT VALID-HANDLE(hEdit) OR hEdit:NAME <> cColumn OR hEdit:EXTENT <> 0
   OR LOOKUP(hEdit:DATA-TYPE, "CHARACTER,INTEGER") = 0 THEN RUN Fail("shape").
DO i = 1 TO 256:
    cInfo = hBuffer:INDEX-INFORMATION(i).
    IF cInfo = ? OR ENTRY(1, cInfo) = cIndex THEN LEAVE.
END.
IF cInfo = ? OR NUM-ENTRIES(cInfo) <> 6 OR ENTRY(1, cInfo) <> cIndex
   OR ENTRY(2, cInfo) <> "1" OR ENTRY(3, cInfo) <> "1" OR ENTRY(4, cInfo) <> "0"
   OR ENTRY(5, cInfo) <> cKeyField THEN RUN Fail("shape").
cPredicate = "WHERE cide_edit." + cKeyField + " = " + cKey + " USE-INDEX " + cIndex.
CREATE ttImage.
hRaw = BUFFER ttImage:BUFFER-FIELD("payload").

IF cMode = "preview" THEN DO:
    lOk = hBuffer:FIND-UNIQUE(cPredicate, NO-LOCK, NO-WAIT) NO-ERROR.
    IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN RUN Fail("conflict").
    lOk = hBuffer:RAW-TRANSFER(TRUE, hRaw) NO-ERROR.
    IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN RUN Fail("shape").
    ASSIGN cImage = RawHex(ttImage.payload)
           cValue = FieldValue(hEdit)
           cRow = STRING(hBuffer:ROWID).
END.
ELSE DO:
    cValue = TextFromHex(cNext).
    IF cNext <> "-" AND (cValue = ? OR TextHex(cValue) <> cNext) THEN RUN Fail("shape").
    IF hEdit:DATA-TYPE = "INTEGER" AND cValue <> ? THEN DO:
        iNext = INTEGER(cValue) NO-ERROR.
        IF ERROR-STATUS:ERROR OR iNext = ? OR TRIM(STRING(iNext)) <> cValue THEN RUN Fail("shape").
    END.
    DO TRANSACTION ON ERROR UNDO, LEAVE ON STOP UNDO, LEAVE:
        lOk = hBuffer:FIND-UNIQUE(cPredicate, EXCLUSIVE-LOCK, NO-WAIT) NO-ERROR.
        cStatus = "conflict".
        IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN UNDO, LEAVE.
        lOk = hBuffer:RAW-TRANSFER(TRUE, hRaw) NO-ERROR.
        IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN UNDO, LEAVE.
        ASSIGN cImage = RawHex(ttImage.payload)
               cRow = STRING(hBuffer:ROWID).
        IF cRow <> cOldRow OR cImage = ? OR cImage <> cOldImage THEN UNDO, LEAVE.
        cStatus = "write".
        IF cValue = ? THEN hEdit:BUFFER-VALUE = ? NO-ERROR.
        ELSE IF hEdit:DATA-TYPE = "INTEGER" THEN hEdit:BUFFER-VALUE = iNext NO-ERROR.
        ELSE hEdit:BUFFER-VALUE = cValue NO-ERROR.
        IF ERROR-STATUS:ERROR THEN UNDO, LEAVE.
        lOk = hBuffer:BUFFER-VALIDATE() NO-ERROR.
        IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN UNDO, LEAVE.
        lOk = hBuffer:RAW-TRANSFER(TRUE, hRaw) NO-ERROR.
        IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN UNDO, LEAVE.
        ASSIGN cAfter = FieldValue(hEdit)
               cImage = RawHex(ttImage.payload).
        IF cAfter = ? OR cAfter <> cNext OR cImage = ? THEN UNDO, LEAVE.
        lCommitted = TRUE.
    END.
    IF NOT lCommitted THEN RUN Fail(cStatus).
    cValue = cAfter.
END.

IF cValue = ? OR cImage = ? OR cRow = ? THEN RUN Fail("shape").
PUT STREAM outputFile UNFORMATTED "ROW|" cRow SKIP "IMAGE|" cImage SKIP "VALUE|" cValue SKIP "END" SKIP.
OUTPUT STREAM outputFile CLOSE.
DELETE OBJECT hBuffer.
QUIT.

PROCEDURE Fail:
    DEFINE INPUT PARAMETER pcError AS CHARACTER NO-UNDO.
    PUT STREAM outputFile UNFORMATTED "ERROR|" pcError SKIP.
    OUTPUT STREAM outputFile CLOSE.
    QUIT.
END PROCEDURE.

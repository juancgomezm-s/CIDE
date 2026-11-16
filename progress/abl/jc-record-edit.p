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
DEFINE VARIABLE cPair AS CHARACTER NO-UNDO.
DEFINE VARIABLE cFieldName AS CHARACTER NO-UNDO.
DEFINE VARIABLE cSeen AS CHARACTER NO-UNDO.
DEFINE VARIABLE cInitial AS CHARACTER CASE-SENSITIVE NO-UNDO.
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
DEFINE VARIABLE lReadOnly AS LOGICAL NO-UNDO.
DEFINE VARIABLE lTriggers AS LOGICAL NO-UNDO.

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

FUNCTION ImageFromHex RETURNS RAW (INPUT pcHex AS CHARACTER):
    DEFINE VARIABLE dataBytes AS RAW NO-UNDO.
    DEFINE VARIABLE idx AS INTEGER NO-UNDO.
    DEFINE VARIABLE hi AS INTEGER NO-UNDO.
    DEFINE VARIABLE lo AS INTEGER NO-UNDO.
    DEFINE VARIABLE digits AS CHARACTER NO-UNDO INITIAL "0123456789ABCDEF".
    IF LENGTH(pcHex) < 2 OR LENGTH(pcHex) > 16384 OR LENGTH(pcHex) MODULO 2 <> 0 THEN RETURN ?.
    LENGTH(dataBytes) = INTEGER(LENGTH(pcHex) / 2).
    DO idx = 1 TO LENGTH(pcHex) BY 2:
        ASSIGN hi = INDEX(digits, SUBSTRING(pcHex, idx, 1))
               lo = INDEX(digits, SUBSTRING(pcHex, idx + 1, 1)).
        IF hi = 0 OR lo = 0 THEN RETURN ?.
        PUT-BYTE(dataBytes, INTEGER((idx + 1) / 2)) = (hi - 1) * 16 + lo - 1.
    END.
    RETURN dataBytes.
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
IF cMode BEGINS "trigger-" THEN DO:
    cMode = SUBSTRING(cMode, 9).
    IF LOOKUP(cMode, "apply,insert,delete") = 0 THEN RUN Fail("shape").
    IF OS-GETENV("CIDE_RECORD_PROPATH") = ? OR OS-GETENV("CIDE_RECORD_PROPATH") = "" THEN RUN Fail("triggers").
    PROPATH = OS-GETENV("CIDE_RECORD_PROPATH") + "," + PROPATH.
    lTriggers = TRUE.
END.
IF LOOKUP(cMode, "preview,apply,inspect,insert,delete,recreate") = 0 THEN RUN Fail("shape").
lReadOnly = LOOKUP(cMode, "preview,inspect") > 0.
IF cVersion <> PROVERSION THEN RUN Fail("version").
IF NUM-DBS <> 1 OR LDBNAME(1) <> cDb THEN RUN Fail("connection").
IF (INDEX(DBRESTRICTIONS(1), "READ-ONLY") > 0) <> lReadOnly THEN RUN Fail("connection").
IF NOT SafeName(cDb) OR NOT SafeName(cTable) OR NOT SafeName(cIndex) OR NOT SafeName(cKeyField)
   OR NOT SafeName(cColumn) OR cColumn = cKeyField THEN RUN Fail("shape").
iKey = INTEGER(cKey) NO-ERROR.
IF ERROR-STATUS:ERROR OR iKey = ? OR TRIM(STRING(iKey)) <> cKey THEN RUN Fail("shape").

CREATE BUFFER hBuffer FOR TABLE (cDb + "." + cTable) BUFFER-NAME "cide_edit" NO-ERROR.
IF NOT VALID-HANDLE(hBuffer) THEN RUN Fail("shape").
lOk = hBuffer:DISABLE-DUMP-TRIGGERS() NO-ERROR.
IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN RUN Fail("triggers").
IF NOT lReadOnly AND NOT lTriggers THEN DO:
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

IF lReadOnly THEN DO:
    lOk = hBuffer:FIND-UNIQUE(cPredicate, NO-LOCK, NO-WAIT) NO-ERROR.
    IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN DO:
        IF cMode = "inspect" AND ERROR-STATUS:NUM-MESSAGES = 1
           AND ERROR-STATUS:GET-NUMBER(1) = 138 THEN RUN AbsentRow.
        RUN Fail("conflict").
    END.
    lOk = hBuffer:RAW-TRANSFER(TRUE, hRaw) NO-ERROR.
    IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN RUN Fail("shape").
    ASSIGN cImage = RawHex(ttImage.payload)
           cValue = FieldValue(hEdit)
           cRow = STRING(hBuffer:ROWID).
END.
ELSE IF cMode = "apply" THEN DO:
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
ELSE DO:
    DO TRANSACTION ON ERROR UNDO, LEAVE ON STOP UNDO, LEAVE:
        lOk = hBuffer:FIND-UNIQUE(cPredicate, EXCLUSIVE-LOCK, NO-WAIT) NO-ERROR.
        cStatus = "conflict".
        IF cMode = "delete" THEN DO:
            IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN UNDO, LEAVE.
            lOk = hBuffer:RAW-TRANSFER(TRUE, hRaw) NO-ERROR.
            IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN UNDO, LEAVE.
            IF STRING(hBuffer:ROWID) <> cOldRow OR RawHex(ttImage.payload) <> cOldImage THEN UNDO, LEAVE.
            cStatus = "write".
            lOk = hBuffer:BUFFER-DELETE() NO-ERROR.
            IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN UNDO, LEAVE.
        END.
        ELSE DO:
            IF lOk = TRUE OR ERROR-STATUS:NUM-MESSAGES <> 1
               OR ERROR-STATUS:GET-NUMBER(1) <> 138 THEN UNDO, LEAVE.
            cStatus = "write".
            lOk = hBuffer:BUFFER-CREATE() NO-ERROR.
            IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN UNDO, LEAVE.
            IF cMode = "recreate" THEN DO:
                ttImage.payload = ImageFromHex(cOldImage).
                IF ttImage.payload = ? THEN UNDO, LEAVE.
                lOk = hBuffer:RAW-TRANSFER(FALSE, hRaw) NO-ERROR.
                IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN UNDO, LEAVE.
            END.
            ELSE DO:
                hField = hBuffer:BUFFER-FIELD(cKeyField).
                hField:BUFFER-VALUE = iKey NO-ERROR.
                IF ERROR-STATUS:ERROR OR NUM-ENTRIES(cNext) > 12 THEN UNDO, LEAVE.
                DO i = 1 TO NUM-ENTRIES(cNext):
                    cPair = ENTRY(i, cNext).
                    IF NUM-ENTRIES(cPair, ":") <> 2 THEN UNDO, LEAVE.
                    ASSIGN cFieldName = ENTRY(1, cPair, ":")
                           cInitial = ENTRY(2, cPair, ":").
                    IF NOT SafeName(cFieldName) OR cFieldName = cKeyField
                       OR LOOKUP(cFieldName, cSeen) <> 0 THEN UNDO, LEAVE.
                    cSeen = cSeen + (IF cSeen = "" THEN "" ELSE ",") + cFieldName.
                    hField = hBuffer:BUFFER-FIELD(cFieldName) NO-ERROR.
                    IF NOT VALID-HANDLE(hField) OR hField:NAME <> cFieldName OR hField:EXTENT <> 0 THEN UNDO, LEAVE.
                    cValue = TextFromHex(cInitial).
                    IF cValue = ? OR TextHex(cValue) <> cInitial THEN UNDO, LEAVE.
                    CASE hField:DATA-TYPE:
                        WHEN "CHARACTER" THEN hField:BUFFER-VALUE = cValue NO-ERROR.
                        WHEN "INTEGER" THEN DO:
                            iNext = INTEGER(cValue) NO-ERROR.
                            IF ERROR-STATUS:ERROR OR iNext = ? OR TRIM(STRING(iNext)) <> cValue THEN UNDO, LEAVE.
                            hField:BUFFER-VALUE = iNext NO-ERROR.
                        END.
                        OTHERWISE UNDO, LEAVE.
                    END CASE.
                    IF ERROR-STATUS:ERROR OR FieldValue(hField) <> cInitial THEN UNDO, LEAVE.
                END.
            END.
            hField = hBuffer:BUFFER-FIELD(cKeyField).
            IF hField:BUFFER-VALUE = ? OR INTEGER(hField:BUFFER-VALUE) <> iKey THEN UNDO, LEAVE.
            lOk = hBuffer:BUFFER-VALIDATE() NO-ERROR.
            IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN UNDO, LEAVE.
            lOk = hBuffer:RAW-TRANSFER(TRUE, hRaw) NO-ERROR.
            IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN UNDO, LEAVE.
            ASSIGN cImage = RawHex(ttImage.payload)
                   cValue = FieldValue(hEdit)
                   cRow = STRING(hBuffer:ROWID).
            IF cImage = ? OR cValue = ? OR cRow = ? THEN UNDO, LEAVE.
            IF cMode = "recreate" AND cImage <> cOldImage THEN UNDO, LEAVE.
        END.
        lCommitted = TRUE.
    END.
    IF NOT lCommitted THEN RUN Fail(cStatus).
    IF cMode = "delete" THEN RUN AbsentRow.
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

PROCEDURE AbsentRow:
    PUT STREAM outputFile UNFORMATTED "ABSENT" SKIP "END" SKIP.
    OUTPUT STREAM outputFile CLOSE.
    QUIT.
END PROCEDURE.

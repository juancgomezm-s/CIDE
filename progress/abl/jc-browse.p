/* CIDE record browser. The host starts a separate AVM with a local -RO DB. */
DEFINE STREAM requestInput.
DEFINE STREAM pageOutput.
DEFINE VARIABLE cHeader AS CHARACTER NO-UNDO.
DEFINE VARIABLE cDatabase AS CHARACTER NO-UNDO.
DEFINE VARIABLE cTable AS CHARACTER NO-UNDO.
DEFINE VARIABLE cIndex AS CHARACTER NO-UNDO.
DEFINE VARIABLE cColumns AS CHARACTER NO-UNDO.
DEFINE VARIABLE cOrder AS CHARACTER NO-UNDO.
DEFINE VARIABLE cDescending AS CHARACTER NO-UNDO.
DEFINE VARIABLE cOffset AS CHARACTER NO-UNDO.
DEFINE VARIABLE cFilterColumn AS CHARACTER NO-UNDO.
DEFINE VARIABLE cFilterHex AS CHARACTER NO-UNDO.
DEFINE VARIABLE cFilter AS CHARACTER NO-UNDO.
DEFINE VARIABLE iFilter AS INTEGER NO-UNDO.
DEFINE VARIABLE iFilterInteger AS INTEGER NO-UNDO.
DEFINE VARIABLE iOffset AS INTEGER NO-UNDO.
DEFINE VARIABLE iColumns AS INTEGER NO-UNDO.
DEFINE VARIABLE iScanned AS INTEGER NO-UNDO.
DEFINE VARIABLE iMatches AS INTEGER NO-UNDO.
DEFINE VARIABLE iRows AS INTEGER NO-UNDO.
DEFINE VARIABLE i AS INTEGER NO-UNDO.
DEFINE VARIABLE j AS INTEGER NO-UNDO.
DEFINE VARIABLE cName AS CHARACTER NO-UNDO.
DEFINE VARIABLE cDirection AS CHARACTER NO-UNDO.
DEFINE VARIABLE cKey AS CHARACTER NO-UNDO.
DEFINE VARIABLE cInfo AS CHARACTER NO-UNDO.
DEFINE VARIABLE cPredicate AS CHARACTER NO-UNDO.
DEFINE VARIABLE cValue AS CHARACTER NO-UNDO.
DEFINE VARIABLE cEncoded AS CHARACTER NO-UNDO.
DEFINE VARIABLE cRow AS CHARACTER NO-UNDO.
DEFINE VARIABLE hBuffer AS HANDLE NO-UNDO.
DEFINE VARIABLE hQuery AS HANDLE NO-UNDO.
DEFINE VARIABLE hFields AS HANDLE EXTENT 12 NO-UNDO.
DEFINE VARIABLE lRead AS LOGICAL NO-UNDO.
DEFINE VARIABLE lOk AS LOGICAL NO-UNDO.
DEFINE VARIABLE lMore AS LOGICAL NO-UNDO.

FUNCTION SafeName RETURNS LOGICAL (INPUT pcName AS CHARACTER):
    DEFINE VARIABLE ix AS INTEGER NO-UNDO.
    DEFINE VARIABLE cAlphabet AS CHARACTER NO-UNDO INITIAL "ABCDEFGHIJKLMNOPQRSTUVWXYZ".
    IF pcName = ? OR LENGTH(pcName) = 0 OR LENGTH(pcName) > 64 THEN RETURN FALSE.
    IF INDEX(cAlphabet, CAPS(SUBSTRING(pcName, 1, 1))) = 0 THEN RETURN FALSE.
    DO ix = 1 TO LENGTH(pcName):
        IF INDEX(cAlphabet + "0123456789_-", CAPS(SUBSTRING(pcName, ix, 1))) = 0 THEN RETURN FALSE.
    END.
    RETURN TRUE.
END FUNCTION.

FUNCTION FromHex RETURNS CHARACTER (INPUT pcHex AS CHARACTER):
    DEFINE VARIABLE mText AS MEMPTR NO-UNDO.
    DEFINE VARIABLE cResult AS CHARACTER NO-UNDO.
    DEFINE VARIABLE ix AS INTEGER NO-UNDO.
    DEFINE VARIABLE hi AS INTEGER NO-UNDO.
    DEFINE VARIABLE lo AS INTEGER NO-UNDO.
    DEFINE VARIABLE cDigits AS CHARACTER NO-UNDO INITIAL "0123456789ABCDEF".
    IF LENGTH(pcHex) > 512 OR LENGTH(pcHex) MODULO 2 <> 0 THEN RETURN ?.
    IF pcHex = "" THEN RETURN "".
    SET-SIZE(mText) = INTEGER(LENGTH(pcHex) / 2) + 1.
    PUT-BYTE(mText, GET-SIZE(mText)) = 0.
    DO ix = 1 TO LENGTH(pcHex) BY 2:
        ASSIGN hi = INDEX(cDigits, SUBSTRING(pcHex, ix, 1))
               lo = INDEX(cDigits, SUBSTRING(pcHex, ix + 1, 1)).
        IF hi = 0 OR lo = 0 OR (hi = 1 AND lo = 1) THEN DO:
            SET-SIZE(mText) = 0.
            RETURN ?.
        END.
        PUT-BYTE(mText, INTEGER((ix + 1) / 2)) = (hi - 1) * 16 + lo - 1.
    END.
    cResult = GET-STRING(mText, 1).
    SET-SIZE(mText) = 0.
    RETURN cResult.
END FUNCTION.

FUNCTION ToHex RETURNS CHARACTER (INPUT pcText AS CHARACTER):
    DEFINE VARIABLE mText AS MEMPTR NO-UNDO.
    DEFINE VARIABLE cResult AS CHARACTER NO-UNDO INITIAL "".
    DEFINE VARIABLE cDigits AS CHARACTER NO-UNDO INITIAL "0123456789ABCDEF".
    DEFINE VARIABLE ix AS INTEGER NO-UNDO.
    DEFINE VARIABLE iLength AS INTEGER NO-UNDO.
    DEFINE VARIABLE iByte AS INTEGER NO-UNDO.
    IF pcText = ? THEN RETURN "-".
    iLength = LENGTH(pcText, "RAW").
    IF iLength > 1024 THEN RETURN "!".
    SET-SIZE(mText) = iLength + 1.
    PUT-STRING(mText, 1) = pcText.
    DO ix = 1 TO iLength:
        iByte = GET-BYTE(mText, ix).
        cResult = cResult + SUBSTRING(cDigits, INTEGER(TRUNCATE(iByte / 16, 0)) + 1, 1)
            + SUBSTRING(cDigits, (iByte MODULO 16) + 1, 1).
    END.
    SET-SIZE(mText) = 0.
    RETURN cResult.
END FUNCTION.

DO ON ERROR UNDO, LEAVE ON ENDKEY UNDO, LEAVE:
    INPUT STREAM requestInput FROM VALUE(SESSION:PARAMETER) NO-CONVERT.
    IMPORT STREAM requestInput UNFORMATTED cHeader.
    IMPORT STREAM requestInput UNFORMATTED cDatabase.
    IMPORT STREAM requestInput UNFORMATTED cTable.
    IMPORT STREAM requestInput UNFORMATTED cIndex.
    IMPORT STREAM requestInput UNFORMATTED cColumns.
    IMPORT STREAM requestInput UNFORMATTED cOrder.
    IMPORT STREAM requestInput UNFORMATTED cDescending.
    IMPORT STREAM requestInput UNFORMATTED cOffset.
    IMPORT STREAM requestInput UNFORMATTED cFilterColumn.
    IMPORT STREAM requestInput UNFORMATTED cFilterHex.
    INPUT STREAM requestInput CLOSE.
    lRead = TRUE.
END.
IF NOT lRead OR cHeader <> "JCIDE-BROWSE/1" THEN QUIT.

OUTPUT STREAM pageOutput TO VALUE(SUBSTRING(SESSION:PARAMETER, 1, R-INDEX(SESSION:PARAMETER, ".")) + "out") NO-CONVERT.
PUT STREAM pageOutput UNFORMATTED cHeader SKIP "AVM|" PROVERSION SKIP.

IF NUM-DBS <> 1 OR LDBNAME(1) <> cDatabase THEN RUN Fail("readonly").
IF OS-GETENV("CIDE_RECORD_ACTIVE") = "1" THEN DO:
    IF INDEX(DBRESTRICTIONS(1), "READ-ONLY") > 0 THEN RUN Fail("readonly").
END.
ELSE IF INDEX(DBRESTRICTIONS(1), "READ-ONLY") = 0 THEN RUN Fail("readonly").
IF NOT SafeName(cDatabase) OR NOT SafeName(cTable) OR NOT SafeName(cIndex) THEN RUN Fail("shape").

ASSIGN iColumns = NUM-ENTRIES(cColumns)
       iFilter = INTEGER(cFilterColumn)
       iOffset = INTEGER(cOffset) NO-ERROR.
IF ERROR-STATUS:ERROR OR iColumns < 1 OR iColumns > EXTENT(hFields)
   OR iFilter = ? OR iFilter < 0 OR iFilter > iColumns OR iOffset = ? OR iOffset < 0 OR iOffset > 2000
   OR (cDescending <> "0" AND cDescending <> "1") THEN RUN Fail("shape").

CREATE BUFFER hBuffer FOR TABLE (cDatabase + "." + cTable) BUFFER-NAME "cide_browse" NO-ERROR.
IF NOT VALID-HANDLE(hBuffer) THEN RUN Fail("shape").
lOk = hBuffer:DISABLE-DUMP-TRIGGERS() NO-ERROR.
IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN RUN Fail("triggers").
DO i = 1 TO iColumns:
    cName = ENTRY(i, cColumns).
    IF NOT SafeName(cName) THEN RUN Fail("shape").
    hFields[i] = hBuffer:BUFFER-FIELD(cName) NO-ERROR.
    IF NOT VALID-HANDLE(hFields[i]) OR hFields[i]:NAME <> cName OR hFields[i]:EXTENT <> 0
       OR LOOKUP(hFields[i]:DATA-TYPE, "CHARACTER,INTEGER,DECIMAL,DATE,DATETIME,DATETIME-TZ,INT64,LOGICAL") = 0
    THEN RUN Fail("shape").
END.

IF iFilter > 0 THEN DO:
    cFilter = FromHex(cFilterHex).
    IF cFilter = ? THEN RUN Fail("filter").
    CASE hFields[iFilter]:DATA-TYPE:
        WHEN "INTEGER" THEN DO:
            iFilterInteger = INTEGER(cFilter) NO-ERROR.
            IF ERROR-STATUS:ERROR OR iFilterInteger = ? THEN RUN Fail("filter").
        END.
        WHEN "CHARACTER" THEN DO: END.
        OTHERWISE RUN Fail("filter").
    END CASE.
END.

/* Recheck the chosen primary index against the live buffer before using it. */
DO i = 1 TO 256:
    cInfo = hBuffer:INDEX-INFORMATION(i).
    IF cInfo = ? OR ENTRY(1, cInfo) = cIndex THEN LEAVE.
END.
IF cInfo = ? OR ENTRY(1, cInfo) <> cIndex OR NUM-ENTRIES(cInfo) <> 4 + 2 * NUM-ENTRIES(cOrder)
   OR ENTRY(2, cInfo) <> "1" OR ENTRY(3, cInfo) <> "1" OR ENTRY(4, cInfo) <> "0"
THEN RUN Fail("shape").
cPredicate = "FOR EACH cide_browse USE-INDEX " + cIndex + " NO-LOCK".
DO i = 1 TO NUM-ENTRIES(cOrder):
    ASSIGN cKey = ENTRY(i, cOrder)
           cName = ENTRY(1, cKey, ":")
           j = 3 + 2 * i.
    IF NUM-ENTRIES(cKey, ":") <> 2 OR NOT SafeName(cName) THEN RUN Fail("shape").
    cDirection = ENTRY(2, cKey, ":").
    IF cName <> ENTRY(j, cInfo) OR (cDirection <> "0" AND cDirection <> "1")
       OR cDirection = ENTRY(j + 1, cInfo) THEN RUN Fail("shape").
    cPredicate = cPredicate + " BY cide_browse." + cName
        + (IF cDirection = cDescending THEN " DESCENDING" ELSE "").
END.

CREATE QUERY hQuery.
hQuery:SET-BUFFERS(hBuffer).
lOk = hQuery:QUERY-PREPARE(cPredicate) NO-ERROR.
IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN RUN Fail("query").
lOk = hQuery:QUERY-OPEN() NO-ERROR.
IF lOk <> TRUE OR ERROR-STATUS:ERROR THEN RUN Fail("query").

REPEAT:
    lOk = hQuery:GET-NEXT(NO-LOCK) NO-ERROR.
    IF ERROR-STATUS:ERROR THEN RUN Fail("query").
    IF hQuery:QUERY-OFF-END THEN LEAVE.
    IF lOk <> TRUE THEN RUN Fail("query").
    iScanned = iScanned + 1.
    IF iScanned > 10000 THEN RUN Fail("scan").
    IF iFilter > 0 THEN DO:
        IF hFields[iFilter]:BUFFER-VALUE = ? THEN NEXT.
        IF hFields[iFilter]:DATA-TYPE = "INTEGER" THEN DO:
            IF INTEGER(hFields[iFilter]:BUFFER-VALUE) <> iFilterInteger THEN NEXT.
        END.
        ELSE IF STRING(hFields[iFilter]:BUFFER-VALUE) <> cFilter THEN NEXT.
    END.
    iMatches = iMatches + 1.
    IF iMatches <= iOffset THEN NEXT.
    IF iRows = 20 THEN DO:
        lMore = TRUE.
        LEAVE.
    END.
    cRow = "ROW".
    DO i = 1 TO iColumns:
        IF hFields[i]:BUFFER-VALUE = ? THEN cValue = ?.
        ELSE DO:
            cValue = STRING(hFields[i]:BUFFER-VALUE).
            IF hFields[i]:DATA-TYPE <> "CHARACTER" THEN cValue = TRIM(cValue).
        END.
        cEncoded = ToHex(cValue).
        IF cEncoded = "!" THEN RUN Fail("cell").
        cRow = cRow + "|" + cEncoded.
    END.
    PUT STREAM pageOutput UNFORMATTED cRow SKIP.
    iRows = iRows + 1.
END.

hQuery:QUERY-CLOSE().
DELETE OBJECT hQuery.
DELETE OBJECT hBuffer.
PUT STREAM pageOutput UNFORMATTED "END|" (IF lMore THEN "1" ELSE "0") SKIP.
OUTPUT STREAM pageOutput CLOSE.
QUIT.

PROCEDURE Fail:
    DEFINE INPUT PARAMETER pcCode AS CHARACTER NO-UNDO.
    PUT STREAM pageOutput UNFORMATTED "ERROR|" pcCode SKIP.
    OUTPUT STREAM pageOutput CLOSE.
    QUIT.
END PROCEDURE.

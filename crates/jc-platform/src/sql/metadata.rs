use super::{Engine, Relation, Session};

const MAX_COLUMNS: usize = 256;
const MAX_INDEX_PARTS: usize = 1_024;
const MAX_FOREIGN_KEY_PARTS: usize = 1_024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub(super) sql_type: String,
}

impl Column {
    pub(super) fn binary(&self) -> bool {
        matches!(
            self.sql_type.as_str(),
            "bytea"
                | "binary"
                | "varbinary"
                | "tinyblob"
                | "blob"
                | "mediumblob"
                | "longblob"
                | "bit"
        )
    }

    pub(super) fn integer(&self) -> bool {
        matches!(
            self.sql_type.as_str(),
            "smallint" | "integer" | "bigint" | "tinyint" | "mediumint" | "int"
        )
    }

    pub(super) fn text(&self) -> bool {
        matches!(
            self.sql_type.as_str(),
            "character"
                | "character varying"
                | "text"
                | "char"
                | "varchar"
                | "tinytext"
                | "mediumtext"
                | "longtext"
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexPart {
    pub index: String,
    pub definition: String,
    pub primary: bool,
    pub unique: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForeignKeyPart {
    pub constraint: String,
    pub column: String,
    pub referenced_schema: String,
    pub referenced_table: String,
    pub referenced_column: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationDetails {
    pub columns: Vec<Column>,
    /// Entries are grouped by index and appear in column order.
    pub indexes: Vec<IndexPart>,
    /// Entries are grouped by foreign key and appear in column order.
    pub foreign_keys: Vec<ForeignKeyPart>,
}

pub(super) fn required<const N: usize>(row: Vec<Option<String>>) -> Result<[String; N], String> {
    row.into_iter()
        .collect::<Option<Vec<_>>>()
        .and_then(|items| items.try_into().ok())
        .ok_or_else(|| "El catálogo SQL contiene metadatos incompletos".into())
}

fn yes(value: &str) -> Result<bool, String> {
    match value {
        "YES" => Ok(true),
        "NO" => Ok(false),
        _ => Err("El catálogo SQL devolvió una bandera desconocida".into()),
    }
}

impl Session {
    fn metadata(
        &self,
        statement: &str,
        relation: &Relation,
        fields: usize,
        limit: usize,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Vec<Vec<Option<String>>>, String> {
        let sql = format!("{statement} LIMIT {}", limit + 1);
        let rows = self.connection.query(
            &sql,
            &[relation.schema(), relation.name()],
            fields,
            limit + 1,
            cancelled,
        )?;
        if rows.len() > limit {
            return Err("El objeto supera el límite de metadatos SQL".into());
        }
        Ok(rows)
    }

    pub(super) fn columns(
        &self,
        relation: &Relation,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Vec<Column>, String> {
        let sql = match self.engine {
            Engine::PostgreSql => "
                SELECT x.column_name, format_type(a.atttypid, a.atttypmod), x.is_nullable, x.data_type
                FROM information_schema.columns x
                JOIN pg_namespace n ON n.nspname = x.table_schema
                JOIN pg_class c ON c.relnamespace = n.oid AND c.relname = x.table_name
                JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = x.ordinal_position
                WHERE x.table_schema = ? AND x.table_name = ?
                ORDER BY x.ordinal_position",
            Engine::MySql => "
                SELECT column_name, column_type, is_nullable, data_type
                FROM information_schema.columns
                WHERE CAST(table_schema AS BINARY) = CAST(? AS BINARY)
                  AND CAST(table_name AS BINARY) = CAST(? AS BINARY)
                ORDER BY ordinal_position",
        };
        self.metadata(sql, relation, 4, MAX_COLUMNS, cancelled)?
            .into_iter()
            .map(|row| {
                let [name, data_type, nullable, sql_type] = required(row)?;
                Ok(Column {
                    name,
                    data_type,
                    nullable: yes(&nullable)?,
                    sql_type,
                })
            })
            .collect()
    }

    pub(super) fn primary_key(
        &self,
        relation: &Relation,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Vec<String>, String> {
        if relation.kind() == super::RelationKind::View {
            return Ok(Vec::new());
        }
        let sql = match self.engine {
            Engine::PostgreSql => {
                "
                SELECT a.attname
                FROM pg_index i
                JOIN pg_class c ON c.oid = i.indrelid
                JOIN pg_namespace n ON n.oid = c.relnamespace
                JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS x(attnum, ordinal)
                    ON x.ordinal <= i.indnkeyatts
                JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = x.attnum
                WHERE n.nspname = ? AND c.relname = ?
                  AND i.indisprimary AND i.indisvalid
                ORDER BY x.ordinal"
            }
            Engine::MySql => {
                "
                SELECT column_name FROM information_schema.statistics
                WHERE CAST(table_schema AS BINARY) = CAST(? AS BINARY)
                  AND CAST(table_name AS BINARY) = CAST(? AS BINARY)
                  AND index_name = 'PRIMARY'
                ORDER BY seq_in_index"
            }
        };
        self.metadata(sql, relation, 1, MAX_COLUMNS, cancelled)?
            .into_iter()
            .map(|row| required(row).map(|[name]| name))
            .collect()
    }

    pub(super) fn indexes(
        &self,
        relation: &Relation,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Vec<IndexPart>, String> {
        let sql = match self.engine {
            Engine::PostgreSql => {
                "
                SELECT ic.relname, pg_get_indexdef(i.indexrelid, x.ordinal, true),
                       CASE WHEN i.indisprimary THEN 'YES' ELSE 'NO' END,
                       CASE WHEN i.indisunique THEN 'YES' ELSE 'NO' END
                FROM pg_index i
                JOIN pg_class c ON c.oid = i.indrelid
                JOIN pg_class ic ON ic.oid = i.indexrelid
                JOIN pg_namespace n ON n.oid = c.relnamespace
                CROSS JOIN LATERAL generate_series(1, i.indnatts) AS x(ordinal)
                WHERE n.nspname = ? AND c.relname = ? AND i.indisvalid
                ORDER BY ic.relname, x.ordinal"
            }
            Engine::MySql => {
                "
                SELECT index_name, COALESCE(expression, column_name),
                       CASE WHEN index_name = 'PRIMARY' THEN 'YES' ELSE 'NO' END,
                       CASE WHEN non_unique = 0 THEN 'YES' ELSE 'NO' END
                FROM information_schema.statistics
                WHERE CAST(table_schema AS BINARY) = CAST(? AS BINARY)
                  AND CAST(table_name AS BINARY) = CAST(? AS BINARY)
                ORDER BY index_name, seq_in_index"
            }
        };
        self.metadata(sql, relation, 4, MAX_INDEX_PARTS, cancelled)?
            .into_iter()
            .map(|row| {
                let [index, definition, primary, unique] = required(row)?;
                Ok(IndexPart {
                    index,
                    definition,
                    primary: yes(&primary)?,
                    unique: yes(&unique)?,
                })
            })
            .collect()
    }

    pub(super) fn foreign_keys(
        &self,
        relation: &Relation,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Vec<ForeignKeyPart>, String> {
        let sql = match self.engine {
            Engine::PostgreSql => "
                SELECT con.conname, sa.attname, rn.nspname, rc.relname, ra.attname
                FROM pg_constraint con
                JOIN pg_class sc ON sc.oid = con.conrelid
                JOIN pg_namespace sn ON sn.oid = sc.relnamespace
                JOIN pg_class rc ON rc.oid = con.confrelid
                JOIN pg_namespace rn ON rn.oid = rc.relnamespace
                CROSS JOIN LATERAL unnest(con.conkey, con.confkey)
                    WITH ORDINALITY AS x(source_attnum, ref_attnum, ordinal)
                JOIN pg_attribute sa ON sa.attrelid = sc.oid AND sa.attnum = x.source_attnum
                JOIN pg_attribute ra ON ra.attrelid = rc.oid AND ra.attnum = x.ref_attnum
                WHERE sn.nspname = ? AND sc.relname = ? AND con.contype = 'f'
                ORDER BY con.conname, x.ordinal",
            Engine::MySql => "
                SELECT constraint_name, column_name, referenced_table_schema, referenced_table_name, referenced_column_name
                FROM information_schema.key_column_usage
                WHERE CAST(table_schema AS BINARY) = CAST(? AS BINARY)
                  AND CAST(table_name AS BINARY) = CAST(? AS BINARY)
                  AND referenced_table_schema IS NOT NULL
                ORDER BY constraint_name, ordinal_position",
        };
        self.metadata(sql, relation, 5, MAX_FOREIGN_KEY_PARTS, cancelled)?
            .into_iter()
            .map(|row| {
                let [constraint, column, referenced_schema, referenced_table, referenced_column] =
                    required(row)?;
                Ok(ForeignKeyPart {
                    constraint,
                    column,
                    referenced_schema,
                    referenced_table,
                    referenced_column,
                })
            })
            .collect()
    }
}

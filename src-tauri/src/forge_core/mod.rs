use crate::{AppConfig, DataSourceConfig, OutputConfig};
use docx_rs::{
    BorderType, Docx, LineSpacing, PageMargin, PageOrientationType, Paragraph, Run, Shading,
    ShdType, Table as DocxTable, TableBorder, TableBorderPosition, TableBorders, TableCell,
    TableCellMargins, TableLayoutType, TableRow, WidthType,
};
use mysql::{params, prelude::Queryable, OptsBuilder, Pool};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct ForgeCore;

pub struct GenerateOutput {
    pub schemas: Vec<String>,
    pub output_dir: String,
    pub stdout: String,
}

pub struct RelationTemplateOutput {
    pub file_name: String,
    pub content: String,
    pub relation_count: usize,
}

trait DatabaseInspector {
    fn inspect_schema(&self, schema: &str) -> Result<DatabaseSchema, String>;
}

#[derive(Debug)]
struct DatabaseSchema {
    name: String,
    tables: Vec<Table>,
    relations: Vec<TableRelation>,
}

#[derive(Debug)]
struct Table {
    name: String,
    comment: String,
    columns: Vec<Column>,
    indexes: Vec<Index>,
}

#[derive(Debug)]
struct Column {
    name: String,
    data_type: String,
    nullable: bool,
    default_value: String,
    comment: String,
    key: String,
    extra: String,
}

#[derive(Debug)]
struct Index {
    name: String,
    columns: Vec<String>,
    unique: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum RelationType {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

#[derive(Clone, Debug, PartialEq)]
enum RelationSource {
    DatabaseFk,
    Uploaded,
    Manual,
    Inferred,
}

#[derive(Clone, Debug, PartialEq)]
struct TableRelation {
    id: String,
    name: Option<String>,
    source_schema: Option<String>,
    source_table: String,
    source_column: String,
    target_schema: Option<String>,
    target_table: String,
    target_column: String,
    relation_type: RelationType,
    source: RelationSource,
    confidence: f32,
    description: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExternalRelation {
    id: Option<String>,
    name: Option<String>,
    source_schema: Option<String>,
    source_table: String,
    source_column: String,
    target_schema: Option<String>,
    target_table: String,
    target_column: String,
    relation_type: String,
    source: Option<String>,
    confidence: Option<f32>,
    description: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RelationTemplateItem {
    #[serde(skip_serializing_if = "String::is_empty")]
    _comment: String,
    source_table: String,
    source_column: String,
    target_table: String,
    target_column: String,
    relation_type: String,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f32>,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
}

impl ForgeCore {
    pub fn generate(config: &AppConfig) -> Result<GenerateOutput, String> {
        validate_config(config)?;
        let schemas = config
            .schemas
            .iter()
            .map(|schema| schema.trim())
            .filter(|schema| !schema.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let output_dir = config.output.dir.trim().to_string();
        fs::create_dir_all(&output_dir)
            .map_err(|error| format!("Failed to create output directory: {error}"))?;

        let inspector = database_inspector(&config.database)?;
        let mut generated_files = Vec::new();
        for schema in &schemas {
            let mut database = inspector.inspect_schema(schema)?;
            apply_table_filters(&mut database, config);
            let inferred_relations =
                infer_relations_from_tables(schema, &database.tables, &relation_aliases(config));
            let external_relations = read_external_relations(relation_file_path(config), schema)?;
            database.relations =
                merge_relations(database.relations, external_relations, inferred_relations);
            remove_relations_for_missing_tables(&mut database);
            let path = render_schema(&database, &config.output, &output_dir)?;
            generated_files.push(path.display().to_string());
        }

        if config.output.open_dir {
            let _ = open_path(Path::new(&output_dir));
        }

        Ok(GenerateOutput {
            schemas,
            output_dir,
            stdout: format!(
                "ForgeCore generated {} file(s): {}",
                generated_files.len(),
                generated_files.join(", ")
            ),
        })
    }

    pub fn generate_er_diagram(config: &AppConfig) -> Result<GenerateOutput, String> {
        validate_config(config)?;
        let schemas = config
            .schemas
            .iter()
            .map(|schema| schema.trim())
            .filter(|schema| !schema.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let output_dir = config.output.dir.trim().to_string();
        fs::create_dir_all(&output_dir)
            .map_err(|error| format!("Failed to create output directory: {error}"))?;

        let inspector = database_inspector(&config.database)?;
        let mut generated_files = Vec::new();
        for schema in &schemas {
            let mut database = inspector.inspect_schema(schema)?;
            apply_table_filters(&mut database, config);
            let inferred_relations =
                infer_relations_from_tables(schema, &database.tables, &relation_aliases(config));
            let external_relations = read_external_relations(relation_file_path(config), schema)?;
            database.relations =
                merge_relations(database.relations, external_relations, inferred_relations);
            remove_relations_for_missing_tables(&mut database);
            let path = render_er_diagram(&database, &output_dir)?;
            generated_files.push(path.display().to_string());
        }

        if config.output.open_dir {
            let _ = open_path(Path::new(&output_dir));
        }

        Ok(GenerateOutput {
            schemas,
            output_dir,
            stdout: format!(
                "ForgeCore generated {} ER diagram file(s): {}",
                generated_files.len(),
                generated_files.join(", ")
            ),
        })
    }

    pub fn generate_relation_template(
        config: &AppConfig,
    ) -> Result<RelationTemplateOutput, String> {
        validate_config(config)?;
        let schemas = config
            .schemas
            .iter()
            .map(|schema| schema.trim())
            .filter(|schema| !schema.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let inspector = database_inspector(&config.database)?;
        let mut items = Vec::new();
        for schema in &schemas {
            let mut database = inspector.inspect_schema(schema)?;
            apply_table_filters(&mut database, config);
            let inferred_relations =
                infer_relations_from_tables(schema, &database.tables, &relation_aliases(config));
            let external_relations = read_external_relations(relation_file_path(config), schema)?;
            database.relations =
                merge_relations(database.relations, external_relations, inferred_relations);
            remove_relations_for_missing_tables(&mut database);
            items.extend(relation_template_items(&database));
        }
        let content = serde_json::to_string_pretty(&items)
            .map_err(|error| format!("Failed to serialize relation template: {error}"))?;
        let file_name = if schemas.len() == 1 {
            format!("{}-relations.json", safe_file_name(&schemas[0]))
        } else {
            "schema-forge-relations.json".to_string()
        };

        Ok(RelationTemplateOutput {
            file_name,
            content: format!("{content}\n"),
            relation_count: items.len(),
        })
    }
}

fn database_inspector(source: &DataSourceConfig) -> Result<Box<dyn DatabaseInspector>, String> {
    let url = source.url.trim();
    if url.starts_with("jdbc:mysql://") {
        return Ok(Box::new(MySqlInspector::new(source)));
    }
    if url.starts_with("jdbc:postgresql://") {
        return Ok(Box::new(PostgresInspector::new(source)));
    }
    if url.starts_with("jdbc:oracle:") {
        return Ok(Box::new(OracleInspector::new(source)));
    }
    Err("Unsupported database URL. ForgeCore currently recognizes MySQL, PostgreSQL, and Oracle JDBC URLs.".to_string())
}

struct MySqlInspector {
    source: DataSourceConfig,
}

impl MySqlInspector {
    fn new(source: &DataSourceConfig) -> Self {
        Self {
            source: source.clone(),
        }
    }

    fn pool_for_schema(&self, schema: &str) -> Result<Pool, String> {
        let endpoint = parse_mysql_jdbc_url(&self.source.url)?;
        let builder = OptsBuilder::new()
            .ip_or_hostname(Some(endpoint.host))
            .tcp_port(endpoint.port)
            .user(Some(self.source.username.clone()))
            .pass(Some(self.source.password.clone()))
            .db_name(Some(schema.to_string()));
        Pool::new(builder).map_err(|error| format!("Failed to create MySQL pool: {error}"))
    }
}

struct PostgresInspector {
    source: DataSourceConfig,
}

impl PostgresInspector {
    fn new(source: &DataSourceConfig) -> Self {
        Self {
            source: source.clone(),
        }
    }
}

impl DatabaseInspector for PostgresInspector {
    fn inspect_schema(&self, schema: &str) -> Result<DatabaseSchema, String> {
        let _ = (&self.source, schema);
        Err("ForgeCore PostgreSQL metadata inspection is not implemented yet.".to_string())
    }
}

struct OracleInspector {
    source: DataSourceConfig,
}

impl OracleInspector {
    fn new(source: &DataSourceConfig) -> Self {
        Self {
            source: source.clone(),
        }
    }
}

impl DatabaseInspector for OracleInspector {
    fn inspect_schema(&self, schema: &str) -> Result<DatabaseSchema, String> {
        let _ = (&self.source, schema);
        Err("ForgeCore Oracle metadata inspection is not implemented yet.".to_string())
    }
}

impl DatabaseInspector for MySqlInspector {
    fn inspect_schema(&self, schema: &str) -> Result<DatabaseSchema, String> {
        let pool = self.pool_for_schema(schema)?;
        let mut conn = pool
            .get_conn()
            .map_err(|error| format!("Failed to connect to MySQL schema `{schema}`: {error}"))?;
        let mut tables = conn
            .exec_map(
                r#"
                select table_name, coalesce(table_comment, '')
                  from information_schema.tables
                 where table_schema = :schema
                   and table_type = 'BASE TABLE'
                 order by table_name
                "#,
                params! { "schema" => schema },
                |(name, comment): (String, String)| Table {
                    name,
                    comment,
                    columns: Vec::new(),
                    indexes: Vec::new(),
                },
            )
            .map_err(|error| format!("Failed to read MySQL tables: {error}"))?;

        let columns =
            conn
                .exec_map(
                    r#"
                select table_name,
                       column_name,
                       column_type,
                       is_nullable,
                       coalesce(column_default, ''),
                       coalesce(column_comment, ''),
                       column_key,
                       extra
                  from information_schema.columns
                 where table_schema = :schema
                 order by table_name, ordinal_position
                "#,
                    params! { "schema" => schema },
                    |(
                        table_name,
                        name,
                        data_type,
                        is_nullable,
                        default_value,
                        comment,
                        key,
                        extra,
                    ): (
                        String,
                        String,
                        String,
                        String,
                        String,
                        String,
                        String,
                        String,
                    )| {
                        (
                            table_name,
                            Column {
                                name,
                                data_type,
                                nullable: is_nullable.eq_ignore_ascii_case("YES"),
                                default_value,
                                comment,
                                key,
                                extra,
                            },
                        )
                    },
                )
                .map_err(|error| format!("Failed to read MySQL columns: {error}"))?;

        let indexes = conn
            .exec_map(
                r#"
                select table_name, index_name, column_name, non_unique, seq_in_index
                  from information_schema.statistics
                 where table_schema = :schema
                 order by table_name, index_name, seq_in_index
                "#,
                params! { "schema" => schema },
                |(table_name, index_name, column_name, non_unique, _seq): (
                    String,
                    String,
                    String,
                    u8,
                    u64,
                )| { (table_name, index_name, column_name, non_unique != 0) },
            )
            .map_err(|error| format!("Failed to read MySQL indexes: {error}"))?;

        let mut table_positions = HashMap::new();
        for (idx, table) in tables.iter().enumerate() {
            table_positions.insert(table.name.clone(), idx);
        }
        for (table_name, column) in columns {
            if let Some(idx) = table_positions.get(&table_name) {
                tables[*idx].columns.push(column);
            }
        }
        for (table_name, index_name, column_name, non_unique) in indexes {
            if let Some(idx) = table_positions.get(&table_name) {
                push_index(&mut tables[*idx], index_name, column_name, !non_unique);
            }
        }

        let relations = read_mysql_foreign_keys(&mut conn, schema, &tables)?;

        Ok(DatabaseSchema {
            name: schema.to_string(),
            tables,
            relations,
        })
    }
}

fn read_mysql_foreign_keys(
    conn: &mut mysql::PooledConn,
    schema: &str,
    tables: &[Table],
) -> Result<Vec<TableRelation>, String> {
    let rows = conn
        .exec_map(
            r#"
            select kcu.constraint_name,
                   kcu.table_schema,
                   kcu.table_name,
                   kcu.column_name,
                   kcu.referenced_table_schema,
                   kcu.referenced_table_name,
                   kcu.referenced_column_name
              from information_schema.key_column_usage kcu
             where kcu.table_schema = :schema
               and kcu.referenced_table_name is not null
               and kcu.referenced_column_name is not null
             order by kcu.table_name, kcu.constraint_name, kcu.ordinal_position
            "#,
            params! { "schema" => schema },
            |(
                constraint_name,
                table_schema,
                table_name,
                column_name,
                referenced_table_schema,
                referenced_table_name,
                referenced_column_name,
            ): (
                String,
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            )| {
                (
                    constraint_name,
                    table_schema,
                    table_name,
                    column_name,
                    referenced_table_schema.unwrap_or_default(),
                    referenced_table_name.unwrap_or_default(),
                    referenced_column_name.unwrap_or_default(),
                )
            },
        )
        .map_err(|error| format!("Failed to read MySQL foreign keys: {error}"))?;

    Ok(rows
        .into_iter()
        .map(
            |(
                constraint_name,
                table_schema,
                table_name,
                column_name,
                referenced_table_schema,
                referenced_table_name,
                referenced_column_name,
            )| TableRelation {
                id: relation_id(
                    "fk",
                    &table_name,
                    &column_name,
                    &referenced_table_name,
                    &referenced_column_name,
                ),
                name: Some(constraint_name),
                source_schema: Some(table_schema),
                source_table: table_name.clone(),
                source_column: column_name.clone(),
                target_schema: Some(referenced_table_schema),
                target_table: referenced_table_name,
                target_column: referenced_column_name,
                relation_type: relation_type_for_source_column(tables, &table_name, &column_name),
                source: RelationSource::DatabaseFk,
                confidence: 1.0,
                description: String::new(),
            },
        )
        .collect())
}

type RelationAliases = HashMap<String, String>;

fn infer_relations_from_tables(
    schema: &str,
    tables: &[Table],
    aliases: &RelationAliases,
) -> Vec<TableRelation> {
    let mut relations = Vec::new();
    for table in tables {
        for column in &table.columns {
            for target in infer_targets_for_column(table, column, tables, aliases) {
                relations.push(TableRelation {
                    id: relation_id(
                        "inferred",
                        &table.name,
                        &column.name,
                        &target.table_name,
                        &target.column_name,
                    ),
                    name: None,
                    source_schema: Some(schema.to_string()),
                    source_table: table.name.clone(),
                    source_column: column.name.clone(),
                    target_schema: Some(schema.to_string()),
                    target_table: target.table_name,
                    target_column: target.column_name,
                    relation_type: target.relation_type,
                    source: RelationSource::Inferred,
                    confidence: target.confidence,
                    description: target.description,
                });
            }
        }
    }
    relations
}

struct InferredTarget {
    table_name: String,
    column_name: String,
    relation_type: RelationType,
    confidence: f32,
    description: String,
}

fn infer_targets_for_column(
    source_table: &Table,
    source_column: &Column,
    tables: &[Table],
    aliases: &RelationAliases,
) -> Vec<InferredTarget> {
    let extension_targets =
        infer_extension_table_targets(source_table, source_column, tables, aliases);
    if !extension_targets.is_empty() {
        return extension_targets;
    }
    if is_primary_key(&source_column.key) {
        return Vec::new();
    }
    if let Some(target) = infer_business_key_target(source_table, source_column, tables, aliases) {
        return vec![target];
    }
    if let Some(prefix) = source_column.name.strip_suffix("_id") {
        return infer_named_target(
            source_table,
            source_column,
            tables,
            aliases,
            prefix,
            "id",
            0.86,
        )
        .into_iter()
        .collect();
    }
    if let Some(prefix) = source_column.name.strip_suffix("_code") {
        return infer_named_target(
            source_table,
            source_column,
            tables,
            aliases,
            prefix,
            "code",
            0.82,
        )
        .into_iter()
        .collect();
    }
    Vec::new()
}

fn infer_named_target(
    source_table: &Table,
    source_column: &Column,
    tables: &[Table],
    aliases: &RelationAliases,
    prefix: &str,
    target_column: &str,
    base_confidence: f32,
) -> Option<InferredTarget> {
    let source_type = normalized_type(&source_column.data_type);
    let mut candidates = tables
        .iter()
        .filter(|table| table.name != source_table.name)
        .filter(|table| table_name_matches_prefix(&table.name, prefix, aliases))
        .filter_map(|table| {
            let column = table.columns.iter().find(|column| {
                column.name == target_column
                    && normalized_type(&column.data_type) == source_type
                    && (is_primary_key(&column.key)
                        || has_unique_single_column_index(table, target_column))
            })?;
            let mut confidence = base_confidence;
            if source_has_index(source_table, &source_column.name) {
                confidence += 0.04;
            }
            if is_primary_key(&column.key) {
                confidence += 0.04;
            }
            Some(InferredTarget {
                table_name: table.name.clone(),
                column_name: column.name.clone(),
                relation_type: relation_type_for_source_column(
                    tables,
                    &source_table.name,
                    &source_column.name,
                ),
                confidence: confidence.min(0.94),
                description: format!(
                    "按 {} -> {}.{} 规则推断",
                    source_column.name, table.name, column.name
                ),
            })
        })
        .collect::<Vec<_>>();

    if candidates.len() == 1 {
        candidates.pop()
    } else {
        None
    }
}

fn infer_business_key_target(
    source_table: &Table,
    source_column: &Column,
    tables: &[Table],
    aliases: &RelationAliases,
) -> Option<InferredTarget> {
    let prefix = relation_column_prefix(&source_column.name)?;
    let source_type = normalized_type(&source_column.data_type);
    let mut candidates = tables
        .iter()
        .filter(|table| table.name != source_table.name)
        .filter(|table| !is_extension_table_of(&source_table.name, &table.name, aliases))
        .filter(|table| table_name_matches_business_key_prefix(&table.name, prefix, aliases))
        .filter_map(|table| {
            let column = table.columns.iter().find(|column| {
                column.name == source_column.name
                    && normalized_type(&column.data_type) == source_type
                    && (is_primary_key(&column.key)
                        || has_unique_single_column_index(table, &column.name))
            })?;
            let mut confidence: f32 = 0.84;
            if source_has_index(source_table, &source_column.name) {
                confidence += 0.04;
            }
            if is_primary_key(&column.key) {
                confidence += 0.04;
            }
            Some(InferredTarget {
                table_name: table.name.clone(),
                column_name: column.name.clone(),
                relation_type: relation_type_for_source_column(
                    tables,
                    &source_table.name,
                    &source_column.name,
                ),
                confidence: confidence.min(0.94),
                description: format!(
                    "按同名业务键和表名语义推断：{}.{} -> {}.{}",
                    source_table.name, source_column.name, table.name, column.name
                ),
            })
        })
        .collect::<Vec<_>>();

    if candidates.len() == 1 {
        candidates.pop()
    } else {
        None
    }
}

fn infer_extension_table_targets(
    source_table: &Table,
    source_column: &Column,
    tables: &[Table],
    aliases: &RelationAliases,
) -> Vec<InferredTarget> {
    if !(is_primary_key(&source_column.key)
        || has_unique_single_column_index(source_table, &source_column.name))
    {
        return Vec::new();
    }

    let source_type = normalized_type(&source_column.data_type);
    tables
        .iter()
        .filter(|table| table.name != source_table.name)
        .filter(|table| {
            is_child_table_of(
                &table.name,
                &source_table.name,
                relation_column_prefix(&source_column.name),
                aliases,
            )
        })
        .filter_map(|table| {
            let column = table.columns.iter().find(|column| {
                column.name == source_column.name
                    && normalized_type(&column.data_type) == source_type
            })?;
            let mut confidence: f32 = 0.88;
            if source_has_index(table, &column.name) {
                confidence += 0.04;
            }
            if !column.key.trim().is_empty() {
                confidence += 0.02;
            }
            Some(InferredTarget {
                table_name: table.name.clone(),
                column_name: column.name.clone(),
                relation_type: relation_type_for_target_child_column(table, &column.name),
                confidence: confidence.min(0.94),
                description: format!(
                    "按主表扩展表同名键规则推断：{}.{} -> {}.{}",
                    source_table.name, source_column.name, table.name, column.name
                ),
            })
        })
        .collect()
}

fn merge_relations(
    database_foreign_keys: Vec<TableRelation>,
    external_relations: Vec<TableRelation>,
    inferred_relations: Vec<TableRelation>,
) -> Vec<TableRelation> {
    let mut merged = Vec::new();
    for relation in database_foreign_keys {
        push_relation(&mut merged, relation);
    }
    for relation in external_relations {
        push_relation(&mut merged, relation);
    }
    for relation in inferred_relations {
        if !merged
            .iter()
            .any(|existing| relation_key(existing) == relation_key(&relation))
        {
            merged.push(relation);
        }
    }
    merged
}

fn read_external_relations(
    relation_file: Option<&str>,
    default_schema: &str,
) -> Result<Vec<TableRelation>, String> {
    let Some(relation_file) = relation_file.map(str::trim).filter(|path| !path.is_empty()) else {
        return Ok(Vec::new());
    };
    let raw = fs::read_to_string(relation_file)
        .map_err(|error| format!("Failed to read relation file `{relation_file}`: {error}"))?;
    let external_relations = serde_json::from_str::<Vec<ExternalRelation>>(&raw)
        .map_err(|error| format!("Invalid relation file `{relation_file}`: {error}"))?;
    external_relations
        .into_iter()
        .map(|relation| relation.into_table_relation(default_schema))
        .collect()
}

fn relation_template_items(database: &DatabaseSchema) -> Vec<RelationTemplateItem> {
    let mut relations = database.relations.iter().collect::<Vec<_>>();
    relations.sort_by(|left, right| {
        relation_source_sort_key(&left.source)
            .cmp(&relation_source_sort_key(&right.source))
            .then_with(|| left.source_table.cmp(&right.source_table))
            .then_with(|| left.source_column.cmp(&right.source_column))
            .then_with(|| left.target_table.cmp(&right.target_table))
            .then_with(|| left.target_column.cmp(&right.target_column))
    });
    relations
        .into_iter()
        .map(|relation| RelationTemplateItem {
            _comment: relation_template_comment(relation),
            source_table: relation.source_table.clone(),
            source_column: relation.source_column.clone(),
            target_table: relation.target_table.clone(),
            target_column: relation.target_column.clone(),
            relation_type: relation_type_label(&relation.relation_type).to_string(),
            source: "uploaded".to_string(),
            confidence: (relation.source == RelationSource::Inferred)
                .then_some(relation.confidence),
            description: relation.description.clone(),
        })
        .collect()
}

fn relation_template_comment(relation: &TableRelation) -> String {
    match relation.source {
        RelationSource::DatabaseFk => "数据库真实外键，已导出为可编辑关系。".to_string(),
        RelationSource::Uploaded => "来自已有关系文件，可继续编辑。".to_string(),
        RelationSource::Manual => "手工关系，可继续编辑。".to_string(),
        RelationSource::Inferred => format!(
            "弱关系推断，置信度 {:.2}；请确认后保留，误判可删除。",
            relation.confidence
        ),
    }
}

fn relation_source_sort_key(source: &RelationSource) -> u8 {
    match source {
        RelationSource::DatabaseFk => 0,
        RelationSource::Uploaded | RelationSource::Manual => 1,
        RelationSource::Inferred => 2,
    }
}

fn relation_file_path(config: &AppConfig) -> Option<&str> {
    config
        .relations
        .as_ref()
        .and_then(|relations| relations.file.as_deref())
}

fn relation_aliases(config: &AppConfig) -> RelationAliases {
    config
        .relations
        .as_ref()
        .and_then(|relations| relations.aliases.as_ref())
        .map(|aliases| {
            aliases
                .iter()
                .flat_map(|(canonical, alias_values)| {
                    let canonical = canonical.trim().to_ascii_lowercase();
                    alias_values.iter().filter_map(move |alias| {
                        let alias = alias.trim().to_ascii_lowercase();
                        if canonical.is_empty() || alias.is_empty() {
                            None
                        } else {
                            Some((alias, canonical.clone()))
                        }
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn apply_table_filters(database: &mut DatabaseSchema, config: &AppConfig) {
    let ignore_patterns = ignored_table_patterns(config);
    if ignore_patterns.is_empty() {
        return;
    }

    database
        .tables
        .retain(|table| !matches_any_table_pattern(&table.name, &ignore_patterns));
    remove_relations_for_missing_tables(database);
}

fn ignored_table_patterns(config: &AppConfig) -> Vec<String> {
    config
        .tables
        .as_ref()
        .and_then(|tables| tables.ignore.as_ref())
        .map(|patterns| {
            patterns
                .iter()
                .map(|pattern| pattern.trim())
                .filter(|pattern| !pattern.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn remove_relations_for_missing_tables(database: &mut DatabaseSchema) {
    let table_names = database
        .tables
        .iter()
        .map(|table| table.name.as_str())
        .collect::<Vec<_>>();
    database.relations.retain(|relation| {
        table_names
            .iter()
            .any(|table_name| *table_name == relation.source_table)
            && table_names
                .iter()
                .any(|table_name| *table_name == relation.target_table)
    });
}

fn matches_any_table_pattern(table_name: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .any(|pattern| table_pattern_matches(table_name, pattern))
}

fn table_pattern_matches(table_name: &str, pattern: &str) -> bool {
    let table_name = table_name.to_ascii_lowercase();
    let pattern = pattern.to_ascii_lowercase();
    if !pattern.contains('*') {
        return table_name == pattern;
    }

    let parts = pattern.split('*').collect::<Vec<_>>();
    let mut remaining = table_name.as_str();
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let Some(position) = remaining.find(part) else {
            return false;
        };
        if index == 0 && !pattern.starts_with('*') && position != 0 {
            return false;
        }
        remaining = &remaining[position + part.len()..];
    }
    if !pattern.ends_with('*') {
        if let Some(last_part) = parts.iter().rev().find(|part| !part.is_empty()) {
            return table_name.ends_with(last_part);
        }
    }
    true
}

impl ExternalRelation {
    fn into_table_relation(self, default_schema: &str) -> Result<TableRelation, String> {
        let source = parse_relation_source(self.source.as_deref().unwrap_or("uploaded"))?;
        let relation_type = parse_relation_type(&self.relation_type)?;
        let id = self.id.unwrap_or_else(|| {
            relation_id(
                source.prefix(),
                &self.source_table,
                &self.source_column,
                &self.target_table,
                &self.target_column,
            )
        });
        let confidence = self.confidence.unwrap_or(match source {
            RelationSource::DatabaseFk | RelationSource::Manual => 1.0,
            RelationSource::Uploaded => 0.95,
            RelationSource::Inferred => 0.7,
        });
        Ok(TableRelation {
            id,
            name: self.name,
            source_schema: Some(
                self.source_schema
                    .unwrap_or_else(|| default_schema.to_string()),
            ),
            source_table: self.source_table,
            source_column: self.source_column,
            target_schema: Some(
                self.target_schema
                    .unwrap_or_else(|| default_schema.to_string()),
            ),
            target_table: self.target_table,
            target_column: self.target_column,
            relation_type,
            source,
            confidence,
            description: self.description.unwrap_or_default(),
        })
    }
}

impl RelationSource {
    fn prefix(&self) -> &'static str {
        match self {
            RelationSource::DatabaseFk => "fk",
            RelationSource::Uploaded => "uploaded",
            RelationSource::Manual => "manual",
            RelationSource::Inferred => "inferred",
        }
    }
}

fn parse_relation_source(value: &str) -> Result<RelationSource, String> {
    match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "database_fk" | "fk" => Ok(RelationSource::DatabaseFk),
        "uploaded" => Ok(RelationSource::Uploaded),
        "manual" => Ok(RelationSource::Manual),
        "inferred" => Ok(RelationSource::Inferred),
        _ => Err(format!("Unsupported relation source: {value}")),
    }
}

fn parse_relation_type(value: &str) -> Result<RelationType, String> {
    match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "one-to-one" => Ok(RelationType::OneToOne),
        "one-to-many" => Ok(RelationType::OneToMany),
        "many-to-one" => Ok(RelationType::ManyToOne),
        "many-to-many" => Ok(RelationType::ManyToMany),
        _ => Err(format!("Unsupported relation type: {value}")),
    }
}

fn push_relation(relations: &mut Vec<TableRelation>, relation: TableRelation) {
    if let Some(position) = relations
        .iter()
        .position(|existing| relation_key(existing) == relation_key(&relation))
    {
        if relation_priority(&relation.source) > relation_priority(&relations[position].source) {
            relations[position] = relation;
        }
    } else {
        relations.push(relation);
    }
}

fn relation_key(relation: &TableRelation) -> (&str, &str, &str, &str) {
    (
        &relation.source_table,
        &relation.source_column,
        &relation.target_table,
        &relation.target_column,
    )
}

fn relation_priority(source: &RelationSource) -> u8 {
    match source {
        RelationSource::DatabaseFk => 4,
        RelationSource::Manual => 3,
        RelationSource::Uploaded => 2,
        RelationSource::Inferred => 1,
    }
}

fn push_index(table: &mut Table, name: String, column: String, unique: bool) {
    if let Some(index) = table.indexes.iter_mut().find(|index| index.name == name) {
        index.columns.push(column);
    } else {
        table.indexes.push(Index {
            name,
            columns: vec![column],
            unique,
        });
    }
}

struct MySqlEndpoint {
    host: String,
    port: u16,
}

fn parse_mysql_jdbc_url(url: &str) -> Result<MySqlEndpoint, String> {
    let without_prefix = url
        .trim()
        .strip_prefix("jdbc:mysql://")
        .ok_or_else(|| "Invalid MySQL JDBC URL.".to_string())?;
    let authority = without_prefix
        .split(['/', '?'])
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Missing host in MySQL JDBC URL.".to_string())?;
    let (host, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        let port = port
            .parse::<u16>()
            .map_err(|_| format!("Invalid MySQL port in JDBC URL: {port}"))?;
        (host.to_string(), port)
    } else {
        (authority.to_string(), 3306)
    };
    Ok(MySqlEndpoint { host, port })
}

fn render_schema(
    database: &DatabaseSchema,
    engine: &OutputConfig,
    output_dir: &str,
) -> Result<PathBuf, String> {
    let file_type = engine.file_type.trim().to_ascii_uppercase();
    let labels = Labels::from_engine(engine)?;
    let base_name = engine
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&database.name);
    match file_type.as_str() {
        "HTML" => write_file(
            output_dir,
            base_name,
            "html",
            &render_html(database, labels),
        ),
        "MD" => write_file(
            output_dir,
            base_name,
            "md",
            &render_markdown(database, labels),
        ),
        "WORD" => write_docx(output_dir, base_name, database, labels),
        _ => Err(format!("Unsupported file type: {}", engine.file_type)),
    }
}

fn render_er_diagram(database: &DatabaseSchema, output_dir: &str) -> Result<PathBuf, String> {
    write_file(
        output_dir,
        &format!("{}-er", database.name),
        "html",
        &render_er_html(database),
    )
}

fn write_file(
    output_dir: &str,
    base_name: &str,
    extension: &str,
    content: &str,
) -> Result<PathBuf, String> {
    let path = Path::new(output_dir).join(format!("{}.{}", safe_file_name(base_name), extension));
    fs::write(&path, content)
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
    Ok(path)
}

fn write_docx(
    output_dir: &str,
    base_name: &str,
    database: &DatabaseSchema,
    labels: Labels,
) -> Result<PathBuf, String> {
    let path = Path::new(output_dir).join(format!("{}.docx", safe_file_name(base_name)));
    let file = File::create(&path)
        .map_err(|error| format!("Failed to create {}: {error}", path.display()))?;
    render_docx(database, labels)
        .build()
        .pack(file)
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
    Ok(path)
}

const ZH_CN_LABELS: &str = include_str!("i18n/zh-CN.json");
const EN_US_LABELS: &str = include_str!("i18n/en-US.json");
const DOC_PRIMARY_COLOR: &str = "17664C";
const DOC_TEXT_COLOR: &str = "17201C";
const DOC_MUTED_COLOR: &str = "68756D";
const DOC_HEADER_FILL: &str = "E7F0EB";
const DOC_BORDER_COLOR: &str = "CCD8D1";
const DOC_PAGE_WIDTH_LANDSCAPE: u32 = 16838;
const DOC_PAGE_HEIGHT_LANDSCAPE: u32 = 11906;
const DOC_TABLE_WIDTH_DXA: usize = 15360;
const DOC_META_WIDTHS: [usize; 2] = [2200, 13160];
const DOC_DIRECTORY_WIDTHS: [usize; 3] = [700, 5200, 9460];
const DOC_COLUMN_WIDTHS: [usize; 8] = [520, 2200, 1900, 900, 760, 1500, 1600, 5980];
const DOC_INDEX_WIDTHS: [usize; 3] = [4200, 1200, 9960];
const ER_NODE_WIDTH: usize = 280;
const ER_NODE_MIN_HEIGHT: usize = 168;
const ER_COLUMN_HEIGHT: usize = 30;
const ER_NODE_TITLE_HEIGHT: usize = 48;
const ER_NODE_COMMENT_LINE_HEIGHT: usize = 24;
const ER_NODE_VERTICAL_PADDING: usize = 34;
const ER_NODE_GAP_X: usize = 80;
const ER_NODE_GAP_Y: usize = 64;
const ER_GRID_MIN_COLUMNS: usize = 3;
const ER_GRID_MAX_COLUMNS: usize = 8;
const ER_START_X: usize = 32;
const ER_START_Y: usize = 96;

#[derive(Clone, Deserialize)]
struct Labels {
    html_lang: String,
    doc_title: String,
    database_name: String,
    document_version: String,
    document_description: String,
    table_directory: String,
    sequence: String,
    table_name: String,
    description: String,
    back_to_index: String,
    data_columns: String,
    column_name: String,
    data_type: String,
    nullable: String,
    primary_key: String,
    default_value: String,
    extra: String,
    indexes: String,
    index_name: String,
    unique: String,
    columns: String,
    yes: String,
    no: String,
}

impl Labels {
    fn from_engine(engine: &OutputConfig) -> Result<Self, String> {
        let language = engine
            .language
            .as_deref()
            .unwrap_or("zh-CN")
            .trim()
            .to_ascii_lowercase();
        let raw = match language.as_str() {
            "en" | "en-us" => EN_US_LABELS,
            "zh" | "zh-cn" => ZH_CN_LABELS,
            _ => ZH_CN_LABELS,
        };
        serde_json::from_str(raw).map_err(|error| format!("Invalid i18n labels: {error}"))
    }

    fn bool(&self, value: bool) -> &str {
        if value {
            &self.yes
        } else {
            &self.no
        }
    }
}

fn render_markdown(database: &DatabaseSchema, labels: Labels) -> String {
    let mut md = format!("<a id=\"index\"></a>\n\n# {}\n\n", labels.doc_title);
    md.push_str(&format!(
        "{}: `{}`\n\n",
        labels.database_name, database.name
    ));
    md.push_str(&format!("{}: 1.0.0\n\n", labels.document_version));
    md.push_str(&format!(
        "{}: Database design document\n\n",
        labels.document_description
    ));
    md.push_str("---\n\n");
    md.push_str(&format!("## {}\n\n", labels.table_directory));
    md.push_str(&format!(
        "| {} | {} | {} |\n",
        labels.sequence, labels.table_name, labels.description
    ));
    md.push_str("| --- | --- | --- |\n");
    for (index, table) in database.tables.iter().enumerate() {
        md.push_str(&format!(
            "| {} | [`{}`](#{}) | {} |\n",
            index + 1,
            table.name,
            anchor_name(&table.name),
            markdown_cell(&table.comment)
        ));
    }
    md.push('\n');

    for table in &database.tables {
        md.push_str(&format!(
            "---\n\n<a id=\"{}\"></a>\n\n## {}: `{}`\n\n",
            anchor_name(&table.name),
            labels.table_name,
            table.name
        ));
        md.push_str(&format!("[{}](#index)\n\n", labels.back_to_index));
        if !table.comment.is_empty() {
            md.push_str(&format!("{}: {}\n\n", labels.description, table.comment));
        }
        md.push_str(&format!("### {}\n\n", labels.data_columns));
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            labels.sequence,
            labels.column_name,
            labels.data_type,
            labels.nullable,
            labels.primary_key,
            labels.default_value,
            labels.extra,
            labels.description
        ));
        md.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
        for (index, column) in table.columns.iter().enumerate() {
            md.push_str(&format!(
                "| {} | `{}` | `{}` | {} | {} | {} | {} | {} |\n",
                index + 1,
                column.name,
                column.data_type,
                labels.bool(column.nullable),
                labels.bool(is_primary_key(&column.key)),
                markdown_cell(&column.default_value),
                markdown_cell(&column.extra),
                markdown_cell(&column.comment)
            ));
        }
        if !table.indexes.is_empty() {
            md.push_str(&format!("\n### {}\n\n", labels.indexes));
            md.push_str(&format!(
                "| {} | {} | {} |\n",
                labels.index_name, labels.unique, labels.columns
            ));
            md.push_str("| --- | --- | --- |\n");
            for index in &table.indexes {
                md.push_str(&format!(
                    "| `{}` | {} | {} |\n",
                    index.name,
                    labels.bool(index.unique),
                    index.columns.join(", ")
                ));
            }
        }
        md.push('\n');
    }
    md
}

fn render_html(database: &DatabaseSchema, labels: Labels) -> String {
    let mut html = format!(
        r#"<!doctype html>
<html lang="{}">
<head>
  <meta charset="utf-8">
  <title>{} - {}</title>
  <style>
    :root {{ color-scheme: light; }}
    body {{ background: #f3f6f4; color: #17201c; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif; line-height: 1.55; margin: 0; }}
    .page {{ margin: 0 auto; max-width: 1180px; padding: 36px 32px 56px; }}
    .doc-header {{ background: #ffffff; border: 1px solid #d7dfda; border-radius: 8px; padding: 24px 28px; }}
    h1 {{ font-size: 30px; line-height: 1.2; margin: 0 0 18px; }}
    h2 {{ border-bottom: 2px solid #17664c; font-size: 22px; margin: 34px 0 12px; padding-bottom: 8px; }}
    h3 {{ font-size: 17px; margin: 20px 0 10px; }}
    .meta {{ display: grid; gap: 8px; margin: 0; }}
    .meta div {{ display: grid; gap: 8px; grid-template-columns: 120px minmax(0, 1fr); }}
    .meta dt {{ color: #5f7067; font-weight: 700; }}
    .meta dd {{ margin: 0; overflow-wrap: anywhere; }}
    .section {{ background: #ffffff; border: 1px solid #d7dfda; border-radius: 8px; margin-top: 22px; padding: 20px 22px; }}
    .section-title {{ align-items: baseline; display: flex; gap: 12px; justify-content: space-between; }}
    .back-link {{ color: #17664c; font-size: 13px; font-weight: 700; text-decoration: none; }}
    table {{ border-collapse: collapse; width: 100%; margin: 12px 0 22px; table-layout: fixed; }}
    th, td {{ border: 1px solid #ccd8d1; padding: 9px 10px; text-align: left; vertical-align: top; word-break: break-word; }}
    th {{ background: #e7f0eb; color: #163d2f; font-weight: 800; }}
    tbody tr:nth-child(even) {{ background: #f8faf9; }}
    code {{ background: #edf4ef; border-radius: 4px; color: #123a2c; padding: 1px 4px; }}
    .muted {{ color: #68756d; }}
    .num {{ width: 58px; }}
    .name {{ width: 20%; }}
    .type {{ width: 18%; }}
    .flag {{ width: 90px; }}
    @media print {{ body {{ background: #ffffff; }} .page {{ max-width: none; padding: 0; }} .doc-header, .section {{ border: 0; }} }}
  </style>
</head>
<body>
  <main class="page">
    <header class="doc-header">
      <h1>{}</h1>
      <dl class="meta">
        <div><dt>{}</dt><dd><code>{}</code></dd></div>
        <div><dt>{}</dt><dd>1.0.0</dd></div>
        <div><dt>{}</dt><dd>Database design document</dd></div>
      </dl>
    </header>
    <section class="section" id="index">
      <h2>{}</h2>
      <table>
        <thead><tr><th class="num">{}</th><th>{}</th><th>{}</th></tr></thead>
        <tbody>
"#,
        labels.html_lang,
        escape_html(&labels.doc_title),
        escape_html(&database.name),
        escape_html(&labels.doc_title),
        escape_html(&labels.database_name),
        escape_html(&database.name),
        escape_html(&labels.document_version),
        escape_html(&labels.document_description),
        escape_html(&labels.table_directory),
        escape_html(&labels.sequence),
        escape_html(&labels.table_name),
        escape_html(&labels.description),
    );
    for (index, table) in database.tables.iter().enumerate() {
        html.push_str(&format!(
            "<tr><td>{}</td><td><a href=\"#{}\"><code>{}</code></a></td><td>{}</td></tr>\n",
            index + 1,
            anchor_name(&table.name),
            escape_html(&table.name),
            escape_html(&table.comment)
        ));
    }
    html.push_str("        </tbody>\n      </table>\n    </section>\n");
    for table in &database.tables {
        html.push_str(&format!(
            "<section class=\"section\" id=\"{}\">\n  <div class=\"section-title\"><h2>{}: <code>{}</code></h2><a class=\"back-link\" href=\"#index\">{}</a></div>\n",
            anchor_name(&table.name),
            escape_html(&labels.table_name),
            escape_html(&table.name),
            escape_html(&labels.back_to_index)
        ));
        if !table.comment.is_empty() {
            html.push_str(&format!(
                "<p><strong>{}:</strong> {}</p>\n",
                escape_html(&labels.description),
                escape_html(&table.comment)
            ));
        }
        html.push_str(&format!(
            "<h3>{}</h3><table><thead><tr><th class=\"num\">{}</th><th class=\"name\">{}</th><th class=\"type\">{}</th><th class=\"flag\">{}</th><th class=\"flag\">{}</th><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>\n",
            escape_html(&labels.data_columns),
            escape_html(&labels.sequence),
            escape_html(&labels.column_name),
            escape_html(&labels.data_type),
            escape_html(&labels.nullable),
            escape_html(&labels.primary_key),
            escape_html(&labels.default_value),
            escape_html(&labels.extra),
            escape_html(&labels.description)
        ));
        for (index, column) in table.columns.iter().enumerate() {
            html.push_str(&format!(
                "<tr><td>{}</td><td><code>{}</code></td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                index + 1,
                escape_html(&column.name),
                escape_html(&column.data_type),
                labels.bool(column.nullable),
                labels.bool(is_primary_key(&column.key)),
                escape_html(&column.default_value),
                escape_html(&column.extra),
                escape_html(&column.comment)
            ));
        }
        html.push_str("</tbody></table>\n");
        if !table.indexes.is_empty() {
            html.push_str(&format!(
                "<h3>{}</h3><table><thead><tr><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>\n",
                escape_html(&labels.indexes),
                escape_html(&labels.index_name),
                escape_html(&labels.unique),
                escape_html(&labels.columns)
            ));
            for index in &table.indexes {
                html.push_str(&format!(
                    "<tr><td><code>{}</code></td><td>{}</td><td>{}</td></tr>\n",
                    escape_html(&index.name),
                    labels.bool(index.unique),
                    escape_html(&index.columns.join(", "))
                ));
            }
            html.push_str("</tbody></table>\n");
        }
        html.push_str("</section>\n");
    }
    html.push_str("  </main>\n</body>\n</html>\n");
    html
}

fn render_er_html(database: &DatabaseSchema) -> String {
    let grid_columns = er_grid_columns(database.tables.len());
    let node_positions = er_node_positions(&database.tables, &database.relations);
    let max_bottom = node_positions
        .values()
        .map(|position| position.y + position.height)
        .max()
        .unwrap_or(0);
    let canvas_width = 32 + grid_columns * ER_NODE_WIDTH + (grid_columns - 1) * ER_NODE_GAP_X + 32;
    let canvas_height = max_bottom + 80;

    let mut html = format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <title>ER Diagram - {}</title>
  <style>
    html, body {{ height: 100%; }}
    body {{ background: #f3f6f4; color: #17201c; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif; margin: 0; overflow: hidden; }}
    .page {{ box-sizing: border-box; display: flex; flex-direction: column; height: 100vh; padding: 14px; }}
    .diagram-header {{ align-items: center; display: flex; flex: 0 0 auto; gap: 18px; justify-content: space-between; margin-bottom: 10px; min-width: 0; }}
    .title-meta {{ min-width: 0; }}
    h1 {{ font-size: 20px; margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
    .subtitle {{ color: #68756d; font-size: 12px; margin: 3px 0 0; }}
    .header-tools {{ align-items: center; display: flex; flex: 0 0 auto; gap: 18px; }}
    .diagram-toolbar {{ align-items: center; display: flex; gap: 8px; }}
    .diagram-toolbar button {{ background: #ffffff; border: 1px solid #c8d2cc; border-radius: 6px; color: #17201c; cursor: pointer; font-size: 13px; font-weight: 800; min-width: 36px; padding: 7px 10px; }}
    .diagram-toolbar button:hover {{ border-color: #17664c; color: #17664c; }}
    .zoom-value {{ color: #425047; font-size: 12px; font-weight: 800; min-width: 48px; text-align: center; }}
    .diagram {{ background: #ffffff; border: 1px solid #d7dfda; border-radius: 8px; flex: 1 1 auto; min-height: 0; overflow: auto; position: relative; width: 100%; }}
    .diagram-content {{ height: {}px; position: relative; transform-origin: 0 0; width: {}px; }}
    svg {{ height: {}px; left: 0; overflow: visible; position: absolute; top: 0; width: {}px; }}
    .relation-line {{ fill: none; opacity: 0.1; stroke-width: 1.6; transition: opacity 140ms ease, stroke-width 140ms ease; }}
    .relation-line.is-highlighted {{ opacity: 0.95; stroke-width: 3; }}
    .relation-line.is-hidden {{ opacity: 0.025; }}
    .relation-marker {{ fill: none; opacity: 0.18; stroke-linecap: round; stroke-width: 2; transition: opacity 140ms ease, stroke-width 140ms ease; }}
    .relation-marker.is-highlighted {{ opacity: 0.95; stroke-width: 3; }}
    .relation-marker.is-hidden {{ opacity: 0.025; }}
    .relation-database-fk {{ stroke: #17664c; }}
    .relation-uploaded {{ stroke: #275d9f; }}
    .relation-manual {{ stroke: #8c4b13; }}
    .relation-inferred {{ stroke: #7d8790; stroke-dasharray: 7 6; }}
    .table-node {{ background: #fbfcfb; border: 1px solid #c8d2cc; border-radius: 8px; box-shadow: 0 10px 26px rgba(27, 45, 36, 0.08); position: absolute; transition: opacity 140ms ease, outline-color 140ms ease; width: {}px; }}
    .table-node.is-related {{ outline: 3px solid rgba(23, 102, 76, 0.2); }}
    .table-node.is-dimmed {{ opacity: 0.38; }}
    .table-title {{ background: #17664c; border-radius: 7px 7px 0 0; color: #ffffff; font-size: 14px; font-weight: 800; line-height: 1.35; overflow-wrap: anywhere; padding: 10px 11px; }}
    .table-comment {{ color: #68756d; font-size: 11px; line-height: 1.35; padding: 8px 11px 0; }}
    .column-list {{ display: grid; padding: 8px 0 10px; }}
    .column-row {{ align-items: center; display: grid; gap: 8px; grid-template-columns: minmax(0, 1fr) auto; min-height: 28px; padding: 0 11px; transition: background 140ms ease; }}
    .column-row.is-relation-endpoint {{ background: #e7f0eb; }}
    .column-name {{ font-size: 12px; font-weight: 750; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
    .column-type {{ color: #68756d; font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
    .badge {{ background: #e7f0eb; border-radius: 5px; color: #17664c; font-size: 10px; font-weight: 850; padding: 2px 5px; }}
    .legend {{ display: flex; flex-wrap: wrap; gap: 12px; }}
    .legend span {{ align-items: center; color: #425047; display: inline-flex; font-size: 12px; font-weight: 750; gap: 6px; }}
    .legend i {{ border-top: 2px solid; display: inline-block; width: 28px; }}
    .legend .database-fk {{ border-color: #17664c; }}
    .legend .uploaded {{ border-color: #275d9f; }}
    .legend .manual {{ border-color: #8c4b13; }}
    .legend .inferred {{ border-color: #7d8790; border-top-style: dashed; }}
    @media (max-width: 980px) {{ .legend {{ display: none; }} }}
  </style>
</head>
<body>
  <main class="page">
    <header class="diagram-header">
      <div class="title-meta">
        <h1>ER Diagram - {}</h1>
        <p class="subtitle">Tables: {} · Relations: {}</p>
      </div>
      <div class="header-tools">
        <div class="legend" aria-label="关系来源图例">
          <span><i class="database-fk"></i>真实外键</span>
          <span><i class="uploaded"></i>上传关系</span>
          <span><i class="manual"></i>手工关系</span>
          <span><i class="inferred"></i>弱关系推断</span>
        </div>
        <div class="diagram-toolbar" aria-label="ER 图缩放控制">
          <button type="button" data-zoom="out">-</button>
          <button type="button" data-zoom="reset">100%</button>
          <button type="button" data-zoom="in">+</button>
          <span class="zoom-value" data-zoom-value>100%</span>
        </div>
      </div>
    </header>
    <section class="diagram">
      <div class="diagram-content" data-diagram-content data-base-width="{}" data-base-height="{}">
        <svg viewBox="0 0 {} {}" aria-hidden="true">
"#,
        escape_html(&database.name),
        canvas_height,
        canvas_width,
        canvas_height,
        canvas_width,
        ER_NODE_WIDTH,
        escape_html(&database.name),
        database.tables.len(),
        database.relations.len(),
        canvas_width,
        canvas_height,
        canvas_width,
        canvas_height
    );

    for (relation_key, relation) in database.relations.iter().enumerate() {
        let Some(source_position) = node_positions.get(&relation.source_table) else {
            continue;
        };
        let Some(target_position) = node_positions.get(&relation.target_table) else {
            continue;
        };
        let source_is_left = source_position.x <= target_position.x;
        let start_x = if source_is_left {
            source_position.x + ER_NODE_WIDTH
        } else {
            source_position.x
        };
        let start_y = er_column_anchor_y(&database.tables, relation, true, source_position);
        let end_x = if source_is_left {
            target_position.x
        } else {
            target_position.x + ER_NODE_WIDTH
        };
        let end_y = er_column_anchor_y(&database.tables, relation, false, target_position);
        let mid_x = (start_x + end_x) / 2;
        html.push_str(&format!(
            "        <path class=\"relation-line {}\" data-relation-key=\"{}\" data-source=\"{}\" data-source-column=\"{}\" data-target=\"{}\" data-target-column=\"{}\" d=\"M {} {} C {} {}, {} {}, {} {}\" />\n",
            relation_css_class(&relation.source),
            relation_key,
            escape_html_attr(&relation.source_table),
            escape_html_attr(&relation.source_column),
            escape_html_attr(&relation.target_table),
            escape_html_attr(&relation.target_column),
            start_x,
            start_y,
            mid_x,
            start_y,
            mid_x,
            end_y,
            end_x,
            end_y
        ));
        html.push_str(&er_endpoint_svg(
            start_x,
            start_y,
            ErEndpointSide::Source,
            relation_source_endpoint_kind(&relation.relation_type),
            relation,
            relation_key,
            source_is_left,
        ));
        html.push_str(&er_endpoint_svg(
            end_x,
            end_y,
            ErEndpointSide::Target,
            relation_target_endpoint_kind(&relation.relation_type),
            relation,
            relation_key,
            !source_is_left,
        ));
    }
    html.push_str("        </svg>\n");

    for table in &database.tables {
        let Some(position) = node_positions.get(&table.name) else {
            continue;
        };
        html.push_str(&format!(
            "        <article class=\"table-node\" data-table=\"{}\" style=\"left: {}px; top: {}px;\">\n          <div class=\"table-title\">{}</div>\n",
            escape_html_attr(&table.name),
            position.x,
            position.y,
            escape_html(&table.name)
        ));
        if !table.comment.is_empty() {
            html.push_str(&format!(
                "          <div class=\"table-comment\">{}</div>\n",
                escape_html(&table.comment)
            ));
        }
        html.push_str("          <div class=\"column-list\">\n");
        for column in &table.columns {
            html.push_str(&format!(
            "            <div class=\"column-row\" data-column=\"{}\"><span class=\"column-name\">{}{}</span><span class=\"column-type\">{}</span></div>\n",
                escape_html_attr(&column.name),
                escape_html(&column.name),
                if is_primary_key(&column.key) { " <span class=\"badge\">PK</span>" } else { "" },
                escape_html(&column.data_type)
            ));
        }
        html.push_str("          </div>\n        </article>\n");
    }

    html.push_str(
        r#"      </div>
    </section>
  </main>
  <script>
    const diagram = document.querySelector('.diagram');
    const diagramContent = document.querySelector('[data-diagram-content]');
    const zoomValue = document.querySelector('[data-zoom-value]');
    const relationLines = [...document.querySelectorAll('.relation-line')];
    const relationElements = [...document.querySelectorAll('.relation-line, .relation-marker')];
    const tableNodes = [...document.querySelectorAll('.table-node')];
    const columnRows = [...document.querySelectorAll('.column-row')];
    let zoom = 1;

    function tableNode(tableName) {
      return tableNodes.find((node) => node.dataset.table === tableName);
    }

    function columnRow(tableName, columnName) {
      const node = tableNode(tableName);
      return node && [...node.querySelectorAll('.column-row')]
        .find((row) => row.dataset.column === columnName);
    }

    function anchorFor(tableName, columnName, side) {
      const node = tableNode(tableName);
      const row = columnRow(tableName, columnName);
      const anchorElement = row || node;
      if (!anchorElement) return null;

      const contentRect = diagramContent.getBoundingClientRect();
      const anchorRect = anchorElement.getBoundingClientRect();
      if (!contentRect.width || !contentRect.height) return null;
      const scaleX = Number(diagramContent.dataset.baseWidth) / contentRect.width;
      const scaleY = Number(diagramContent.dataset.baseHeight) / contentRect.height;
      return {
        x: ((side === 'right' ? anchorRect.right : anchorRect.left) - contentRect.left) * scaleX,
        y: (anchorRect.top + anchorRect.height / 2 - contentRect.top) * scaleY,
      };
    }

    function layoutRelations() {
      relationLines.forEach((line) => {
        const source = line.dataset.source;
        const sourceColumn = line.dataset.sourceColumn;
        const target = line.dataset.target;
        const targetColumn = line.dataset.targetColumn;
        const sourceNode = tableNode(source);
        const targetNode = tableNode(target);
        if (!sourceNode || !targetNode) return;

        const sourceRect = sourceNode.getBoundingClientRect();
        const targetRect = targetNode.getBoundingClientRect();
        const sourceSide = sourceRect.left <= targetRect.left ? 'right' : 'left';
        const targetSide = sourceSide === 'right' ? 'left' : 'right';
        const start = anchorFor(source, sourceColumn, sourceSide);
        const end = anchorFor(target, targetColumn, targetSide);
        if (!start || !end) return;

        const startDirection = sourceSide === 'right' ? 1 : -1;
        const endDirection = targetSide === 'right' ? 1 : -1;
        const handle = Math.max(40, Math.abs(end.x - start.x) * 0.45);
        line.setAttribute(
          'd',
          `M ${start.x} ${start.y} C ${start.x + startDirection * handle} ${start.y}, ${end.x + endDirection * handle} ${end.y}, ${end.x} ${end.y}`,
        );
        relationElements
          .filter((element) => element.classList.contains('relation-marker') && element.dataset.relationKey === line.dataset.relationKey)
          .forEach((marker) => {
            const endpoint = marker.dataset.endpoint === 'source' ? start : end;
            const direction = marker.dataset.endpoint === 'source' ? startDirection : endDirection;
            marker.setAttribute('transform', `translate(${endpoint.x} ${endpoint.y}) scale(${direction} 1)`);
          });
      });
    }

    function applyZoom() {
      const baseWidth = Number(diagramContent.dataset.baseWidth);
      const baseHeight = Number(diagramContent.dataset.baseHeight);
      diagramContent.style.zoom = zoom;
      diagramContent.style.width = `${baseWidth}px`;
      diagramContent.style.height = `${baseHeight}px`;
      zoomValue.textContent = `${Math.round(zoom * 100)}%`;
      requestAnimationFrame(layoutRelations);
    }

    document.querySelectorAll('[data-zoom]').forEach((button) => {
      button.addEventListener('click', () => {
        const action = button.dataset.zoom;
        if (action === 'in') zoom = Math.min(1.8, zoom + 0.1);
        if (action === 'out') zoom = Math.max(0.4, zoom - 0.1);
        if (action === 'reset') zoom = 1;
        applyZoom();
      });
    });
    diagram.addEventListener('wheel', (event) => {
      if (!event.ctrlKey && !event.metaKey) return;
      event.preventDefault();
      zoom = event.deltaY < 0 ? Math.min(1.8, zoom + 0.1) : Math.max(0.4, zoom - 0.1);
      applyZoom();
    }, { passive: false });
    applyZoom();
    window.addEventListener('resize', layoutRelations);

    function setFocus(source, target) {
      relationElements.forEach((line) => {
        const matched = source && target
          ? line.dataset.source === source && line.dataset.target === target
          : line.dataset.source === source || line.dataset.target === source;
        line.classList.toggle('is-highlighted', matched);
        line.classList.toggle('is-hidden', !matched);
      });
      tableNodes.forEach((node) => {
        const related = source && target
          ? node.dataset.table === source || node.dataset.table === target
          : node.dataset.table === source || relationLines.some((line) =>
              (line.dataset.source === source || line.dataset.target === source) &&
              (line.dataset.source === node.dataset.table || line.dataset.target === node.dataset.table)
            );
        node.classList.toggle('is-related', related);
        node.classList.toggle('is-dimmed', !related);
      });
      columnRows.forEach((row) => {
        const tableName = row.closest('.table-node')?.dataset.table;
        const relatedColumn = relationLines.some((line) => {
          const matched = source && target
            ? line.dataset.source === source && line.dataset.target === target
            : line.dataset.source === source || line.dataset.target === source;
          return matched && (
            (line.dataset.source === tableName && line.dataset.sourceColumn === row.dataset.column) ||
            (line.dataset.target === tableName && line.dataset.targetColumn === row.dataset.column)
          );
        });
        row.classList.toggle('is-relation-endpoint', relatedColumn);
      });
    }

    function clearFocus() {
      relationElements.forEach((line) => line.classList.remove('is-highlighted', 'is-hidden'));
      tableNodes.forEach((node) => node.classList.remove('is-related', 'is-dimmed'));
      columnRows.forEach((row) => row.classList.remove('is-relation-endpoint'));
    }

    tableNodes.forEach((node) => {
      node.addEventListener('mouseenter', () => setFocus(node.dataset.table));
      node.addEventListener('mouseleave', clearFocus);
    });
  </script>
</body>
</html>
"#,
    );
    html
}

#[derive(Debug)]
struct ErNodePosition {
    x: usize,
    y: usize,
    height: usize,
}

fn er_node_positions(
    tables: &[Table],
    relations: &[TableRelation],
) -> HashMap<String, ErNodePosition> {
    let mut positions = HashMap::new();
    let ordered_tables = er_ordered_tables(tables, relations);
    let grid_columns = er_grid_columns(tables.len());
    let mut column_bottoms = vec![ER_START_Y; grid_columns];
    for (index, table) in ordered_tables.iter().enumerate() {
        let col = index % grid_columns;
        let x = ER_START_X + col * (ER_NODE_WIDTH + ER_NODE_GAP_X);
        let y = column_bottoms[col];
        let height = er_node_height(table);
        column_bottoms[col] = y + height + ER_NODE_GAP_Y;
        positions.insert(table.name.clone(), ErNodePosition { x, y, height });
    }
    positions
}

fn er_grid_columns(table_count: usize) -> usize {
    if table_count <= ER_GRID_MIN_COLUMNS {
        return ER_GRID_MIN_COLUMNS;
    }
    table_count
        .div_ceil(15)
        .clamp(ER_GRID_MIN_COLUMNS, ER_GRID_MAX_COLUMNS)
}

fn er_ordered_tables<'a>(tables: &'a [Table], relations: &[TableRelation]) -> Vec<&'a Table> {
    let table_by_name = tables
        .iter()
        .map(|table| (table.name.as_str(), table))
        .collect::<HashMap<_, _>>();
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    for relation in relations {
        if !table_by_name.contains_key(relation.source_table.as_str())
            || !table_by_name.contains_key(relation.target_table.as_str())
        {
            continue;
        }
        adjacency
            .entry(relation.source_table.as_str())
            .or_default()
            .push(relation.target_table.as_str());
        adjacency
            .entry(relation.target_table.as_str())
            .or_default()
            .push(relation.source_table.as_str());
    }
    let degree_by_name = adjacency
        .iter()
        .map(|(name, neighbors)| (*name, neighbors.len()))
        .collect::<HashMap<_, _>>();
    for neighbors in adjacency.values_mut() {
        neighbors.sort_unstable();
        neighbors.dedup();
        neighbors
            .sort_by_key(|name| std::cmp::Reverse(degree_by_name.get(*name).copied().unwrap_or(0)));
    }

    let mut seeds = tables
        .iter()
        .map(|table| table.name.as_str())
        .collect::<Vec<_>>();
    seeds.sort_by_key(|name| std::cmp::Reverse(degree_by_name.get(*name).copied().unwrap_or(0)));

    let mut visited = HashMap::new();
    let mut ordered = Vec::new();
    for seed in seeds {
        if visited.contains_key(seed) {
            continue;
        }
        let mut stack = vec![seed];
        while let Some(name) = stack.pop() {
            if visited.insert(name, true).is_some() {
                continue;
            }
            if let Some(table) = table_by_name.get(name) {
                ordered.push(*table);
            }
            if let Some(neighbors) = adjacency.get(name) {
                for neighbor in neighbors.iter().rev() {
                    if !visited.contains_key(*neighbor) {
                        stack.push(*neighbor);
                    }
                }
            }
        }
    }
    ordered
}

fn er_node_height(table: &Table) -> usize {
    let comment_lines = if table.comment.is_empty() {
        0
    } else {
        (table.comment.chars().count() / 24) + 1
    };
    ER_NODE_MIN_HEIGHT.max(
        ER_NODE_TITLE_HEIGHT
            + ER_NODE_VERTICAL_PADDING
            + comment_lines * ER_NODE_COMMENT_LINE_HEIGHT
            + table.columns.len() * ER_COLUMN_HEIGHT,
    )
}

fn er_column_anchor_y(
    tables: &[Table],
    relation: &TableRelation,
    source: bool,
    position: &ErNodePosition,
) -> usize {
    let (table_name, column_name) = if source {
        (&relation.source_table, &relation.source_column)
    } else {
        (&relation.target_table, &relation.target_column)
    };
    let Some(table) = tables.iter().find(|table| table.name == *table_name) else {
        return position.y + position.height / 2;
    };
    let Some(column_index) = table
        .columns
        .iter()
        .position(|column| column.name == *column_name)
    else {
        return position.y + position.height / 2;
    };
    let comment_lines = if table.comment.is_empty() {
        0
    } else {
        (table.comment.chars().count() / 24) + 1
    };
    position.y
        + ER_NODE_TITLE_HEIGHT
        + ER_NODE_VERTICAL_PADDING / 2
        + comment_lines * ER_NODE_COMMENT_LINE_HEIGHT
        + column_index * ER_COLUMN_HEIGHT
        + ER_COLUMN_HEIGHT / 2
}

fn render_docx(database: &DatabaseSchema, labels: Labels) -> Docx {
    let mut doc = Docx::new()
        .page_size(DOC_PAGE_WIDTH_LANDSCAPE, DOC_PAGE_HEIGHT_LANDSCAPE)
        .page_orient(PageOrientationType::Landscape)
        .page_margin(compact_page_margin())
        .add_paragraph(title_heading(&labels.doc_title))
        .add_table(meta_docx_table(database, &labels))
        .add_paragraph(spacer_paragraph(180))
        .add_paragraph(section_heading(&labels.table_directory))
        .add_table(directory_docx_table(database, &labels))
        .add_paragraph(spacer_paragraph(260));
    for table in &database.tables {
        doc = doc.add_paragraph(section_heading(&format!(
            "{}: {}",
            labels.table_name, table.name
        )));
        if !table.comment.is_empty() {
            doc = doc.add_paragraph(paragraph(&format!(
                "{}: {}",
                labels.description, table.comment
            )));
        }
        doc = doc
            .add_paragraph(subsection_heading(&labels.data_columns))
            .add_table(column_docx_table(table, &labels))
            .add_paragraph(spacer_paragraph(160));
        if !table.indexes.is_empty() {
            doc = doc
                .add_paragraph(subsection_heading(&labels.indexes))
                .add_table(index_docx_table(table, &labels))
                .add_paragraph(spacer_paragraph(260));
        } else {
            doc = doc.add_paragraph(spacer_paragraph(160));
        }
    }
    doc
}

fn meta_docx_table(database: &DatabaseSchema, labels: &Labels) -> DocxTable {
    styled_docx_table(
        vec![
            docx_row(
                vec![labels.database_name.to_string(), database.name.clone()],
                false,
                &DOC_META_WIDTHS,
            ),
            docx_row(
                vec![labels.document_version.to_string(), "1.0.0".to_string()],
                false,
                &DOC_META_WIDTHS,
            ),
            docx_row(
                vec![
                    labels.document_description.to_string(),
                    "Database design document".to_string(),
                ],
                false,
                &DOC_META_WIDTHS,
            ),
        ],
        &DOC_META_WIDTHS,
    )
}

fn directory_docx_table(database: &DatabaseSchema, labels: &Labels) -> DocxTable {
    let mut rows = vec![docx_row(
        vec![
            labels.sequence.to_string(),
            labels.table_name.to_string(),
            labels.description.to_string(),
        ],
        true,
        &DOC_DIRECTORY_WIDTHS,
    )];
    for (index, table) in database.tables.iter().enumerate() {
        rows.push(docx_row(
            vec![
                (index + 1).to_string(),
                table.name.clone(),
                table.comment.clone(),
            ],
            false,
            &DOC_DIRECTORY_WIDTHS,
        ));
    }
    styled_docx_table(rows, &DOC_DIRECTORY_WIDTHS)
}

fn column_docx_table(table: &Table, labels: &Labels) -> DocxTable {
    let mut rows = vec![docx_row(
        vec![
            labels.sequence.to_string(),
            labels.column_name.to_string(),
            labels.data_type.to_string(),
            labels.nullable.to_string(),
            labels.primary_key.to_string(),
            labels.default_value.to_string(),
            labels.extra.to_string(),
            labels.description.to_string(),
        ],
        true,
        &DOC_COLUMN_WIDTHS,
    )];
    for (index, column) in table.columns.iter().enumerate() {
        rows.push(docx_row(
            vec![
                (index + 1).to_string(),
                column.name.clone(),
                column.data_type.clone(),
                labels.bool(column.nullable).to_string(),
                labels.bool(is_primary_key(&column.key)).to_string(),
                column.default_value.clone(),
                column.extra.clone(),
                column.comment.clone(),
            ],
            false,
            &DOC_COLUMN_WIDTHS,
        ));
    }
    styled_docx_table(rows, &DOC_COLUMN_WIDTHS)
}

fn index_docx_table(table: &Table, labels: &Labels) -> DocxTable {
    let mut rows = vec![docx_row(
        vec![
            labels.index_name.to_string(),
            labels.unique.to_string(),
            labels.columns.to_string(),
        ],
        true,
        &DOC_INDEX_WIDTHS,
    )];
    for index in &table.indexes {
        rows.push(docx_row(
            vec![
                index.name.clone(),
                labels.bool(index.unique).to_string(),
                index.columns.join(", "),
            ],
            false,
            &DOC_INDEX_WIDTHS,
        ));
    }
    styled_docx_table(rows, &DOC_INDEX_WIDTHS)
}

fn styled_docx_table(rows: Vec<TableRow>, widths: &[usize]) -> DocxTable {
    DocxTable::new(rows)
        .width(DOC_TABLE_WIDTH_DXA, WidthType::Dxa)
        .layout(TableLayoutType::Fixed)
        .set_grid(widths.to_vec())
        .margins(TableCellMargins::new().margin(70, 80, 70, 80))
        .set_borders(light_table_borders())
}

fn docx_row(values: Vec<String>, header: bool, widths: &[usize]) -> TableRow {
    TableRow::new(
        values
            .iter()
            .enumerate()
            .map(|(index, value)| docx_cell(value, header, widths.get(index).copied()))
            .collect(),
    )
}

fn docx_cell(value: &str, header: bool, width: Option<usize>) -> TableCell {
    let run = if header {
        Run::new()
            .add_text(value)
            .bold()
            .size(18)
            .color(DOC_PRIMARY_COLOR)
    } else {
        Run::new().add_text(value).size(17).color(DOC_TEXT_COLOR)
    };
    let mut cell = TableCell::new().add_paragraph(Paragraph::new().add_run(run));
    if let Some(width) = width {
        cell = cell.width(width, WidthType::Dxa);
    }
    if header {
        cell.shading(
            Shading::new()
                .shd_type(ShdType::Clear)
                .fill(DOC_HEADER_FILL),
        )
    } else {
        cell
    }
}

fn compact_page_margin() -> PageMargin {
    PageMargin {
        top: 720,
        right: 720,
        bottom: 720,
        left: 720,
        header: 360,
        footer: 360,
        gutter: 0,
    }
}

fn light_table_borders() -> TableBorders {
    [
        TableBorderPosition::Top,
        TableBorderPosition::Left,
        TableBorderPosition::Bottom,
        TableBorderPosition::Right,
        TableBorderPosition::InsideH,
        TableBorderPosition::InsideV,
    ]
    .into_iter()
    .fold(TableBorders::with_empty(), |borders, position| {
        borders.set(
            TableBorder::new(position)
                .border_type(BorderType::Single)
                .size(2)
                .color(DOC_BORDER_COLOR),
        )
    })
}

fn title_heading(value: &str) -> Paragraph {
    spaced_paragraph(0, 220).add_run(
        Run::new()
            .add_text(value)
            .bold()
            .size(32)
            .color(DOC_PRIMARY_COLOR),
    )
}

fn section_heading(value: &str) -> Paragraph {
    spaced_paragraph(260, 120).add_run(
        Run::new()
            .add_text(value)
            .bold()
            .size(24)
            .color(DOC_PRIMARY_COLOR),
    )
}

fn subsection_heading(value: &str) -> Paragraph {
    spaced_paragraph(140, 80).add_run(
        Run::new()
            .add_text(value)
            .bold()
            .size(20)
            .color(DOC_TEXT_COLOR),
    )
}

fn paragraph(value: &str) -> Paragraph {
    spaced_paragraph(40, 120).add_run(Run::new().add_text(value).size(20).color(DOC_MUTED_COLOR))
}

fn spaced_paragraph(before: u32, after: u32) -> Paragraph {
    Paragraph::new().line_spacing(LineSpacing::new().before(before).after(after))
}

fn spacer_paragraph(after: u32) -> Paragraph {
    Paragraph::new()
        .line_spacing(LineSpacing::new().after(after))
        .add_run(Run::new().add_text(" ").size(2).color("FFFFFF"))
}

fn safe_file_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => ch,
        })
        .collect()
}

fn anchor_name(value: &str) -> String {
    format!(
        "table-{}",
        safe_file_name(value)
            .trim()
            .replace([' ', '.'], "-")
            .to_ascii_lowercase()
    )
}

fn is_primary_key(value: &str) -> bool {
    value.eq_ignore_ascii_case("PRI")
}

fn relation_id(
    prefix: &str,
    source_table: &str,
    source_column: &str,
    target_table: &str,
    target_column: &str,
) -> String {
    safe_file_name(&format!(
        "{}_{}_{}_{}_{}",
        prefix, source_table, source_column, target_table, target_column
    ))
}

fn relation_type_for_source_column(
    tables: &[Table],
    source_table: &str,
    source_column: &str,
) -> RelationType {
    if tables
        .iter()
        .find(|table| table.name == source_table)
        .is_some_and(|table| has_unique_single_column_index(table, source_column))
    {
        RelationType::OneToOne
    } else {
        RelationType::ManyToOne
    }
}

fn relation_type_for_target_child_column(
    target_table: &Table,
    target_column: &str,
) -> RelationType {
    if has_unique_single_column_index(target_table, target_column) {
        RelationType::OneToOne
    } else {
        RelationType::OneToMany
    }
}

fn has_unique_single_column_index(table: &Table, column: &str) -> bool {
    table
        .indexes
        .iter()
        .any(|index| index.unique && index.columns.len() == 1 && index.columns[0] == column)
}

fn source_has_index(table: &Table, column: &str) -> bool {
    table.indexes.iter().any(|index| {
        index
            .columns
            .iter()
            .any(|indexed_column| indexed_column == column)
    })
}

fn normalized_type(value: &str) -> String {
    value
        .trim()
        .split('(')
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase()
}

fn table_name_matches_prefix(table_name: &str, prefix: &str, aliases: &RelationAliases) -> bool {
    let normalized_table = normalize_relation_name(table_name, aliases);
    let normalized_prefix = normalize_relation_name(prefix, aliases);
    normalized_table == normalized_prefix
        || normalized_table == format!("{normalized_prefix}s")
        || normalized_table == format!("{normalized_prefix}es")
}

fn is_extension_table_of(
    table_name: &str,
    base_table_name: &str,
    aliases: &RelationAliases,
) -> bool {
    is_child_table_of(table_name, base_table_name, None, aliases)
}

fn is_child_table_of(
    table_name: &str,
    base_table_name: &str,
    relation_prefix: Option<&str>,
    aliases: &RelationAliases,
) -> bool {
    let table_name = table_name.to_ascii_lowercase();
    let base_table_name = base_table_name.to_ascii_lowercase();
    let Some((child_stem, suffix)) = child_table_stem(&table_name) else {
        return false;
    };
    if child_stem == base_table_name {
        return true;
    }
    if matches!(suffix, "_dtl" | "_detail")
        && base_table_name
            .strip_suffix("_type")
            .is_some_and(|prefix| child_stem == prefix)
    {
        return true;
    }

    child_stem_semantically_matches_parent(child_stem, &base_table_name, relation_prefix, aliases)
}

fn child_table_stem(table_name: &str) -> Option<(&str, &'static str)> {
    [
        "_parameters",
        "_parameter",
        "_attachments",
        "_attachment",
        "_params",
        "_param",
        "_detail",
        "_files",
        "_attach",
        "_dtl",
        "_file",
        "_lang",
        "_i18n",
        "_tl",
    ]
    .iter()
    .find_map(|suffix| table_name.strip_suffix(suffix).map(|stem| (stem, *suffix)))
}

fn child_stem_semantically_matches_parent(
    child_stem: &str,
    parent_table: &str,
    relation_prefix: Option<&str>,
    aliases: &RelationAliases,
) -> bool {
    let mut parent_tokens = expanded_name_tokens(parent_table, aliases);
    let child_tokens = expanded_name_tokens(child_stem, aliases);
    if parent_tokens == child_tokens {
        return true;
    }
    if parent_tokens.last().is_some_and(|token| token == "type") {
        parent_tokens.pop();
        if parent_tokens == child_tokens {
            return true;
        }
    }

    let common_count = parent_tokens
        .iter()
        .filter(|token| child_tokens.contains(token))
        .count();
    let enough_shared_tokens = common_count >= 3 && common_count + 1 >= parent_tokens.len();
    if !enough_shared_tokens {
        return false;
    }

    relation_prefix.is_none_or(|prefix| {
        let prefix_tokens = expanded_name_tokens(prefix, aliases);
        prefix_tokens
            .iter()
            .all(|token| parent_tokens.contains(token) || child_tokens.contains(token))
    })
}

fn relation_column_prefix(column_name: &str) -> Option<&str> {
    column_name
        .strip_suffix("_id")
        .or_else(|| column_name.strip_suffix("_code"))
}

fn table_name_matches_business_key_prefix(
    table_name: &str,
    prefix: &str,
    aliases: &RelationAliases,
) -> bool {
    table_name_matches_prefix(table_name, prefix, aliases)
        || normalize_relation_name(table_name, aliases)
            .ends_with(&normalize_relation_name(prefix, aliases))
}

fn expanded_name_tokens(value: &str, aliases: &RelationAliases) -> Vec<String> {
    value
        .split('_')
        .filter(|token| !token.is_empty())
        .map(|token| expand_relation_token(token, aliases))
        .collect()
}

fn expand_relation_token(token: &str, aliases: &RelationAliases) -> String {
    let token = token.to_ascii_lowercase();
    aliases
        .get(&token)
        .cloned()
        .unwrap_or_else(|| default_relation_token_alias(&token))
}

fn default_relation_token_alias(token: &str) -> String {
    match token {
        "dtl" => "detail".to_string(),
        "param" | "params" => "parameter".to_string(),
        other => other.to_string(),
    }
}

fn normalize_relation_name(value: &str, aliases: &RelationAliases) -> String {
    expanded_name_tokens(value, aliases).join("")
}

fn relation_css_class(source: &RelationSource) -> &'static str {
    match source {
        RelationSource::DatabaseFk => "relation-database-fk",
        RelationSource::Uploaded => "relation-uploaded",
        RelationSource::Manual => "relation-manual",
        RelationSource::Inferred => "relation-inferred",
    }
}

fn relation_type_label(relation_type: &RelationType) -> &'static str {
    match relation_type {
        RelationType::OneToOne => "one-to-one",
        RelationType::OneToMany => "one-to-many",
        RelationType::ManyToOne => "many-to-one",
        RelationType::ManyToMany => "many-to-many",
    }
}

#[derive(Clone, Copy)]
enum ErEndpointKind {
    One,
    Many,
}

#[derive(Clone, Copy)]
enum ErEndpointSide {
    Source,
    Target,
}

fn relation_source_endpoint_kind(relation_type: &RelationType) -> ErEndpointKind {
    match relation_type {
        RelationType::OneToOne | RelationType::OneToMany => ErEndpointKind::One,
        RelationType::ManyToOne | RelationType::ManyToMany => ErEndpointKind::Many,
    }
}

fn relation_target_endpoint_kind(relation_type: &RelationType) -> ErEndpointKind {
    match relation_type {
        RelationType::OneToOne | RelationType::ManyToOne => ErEndpointKind::One,
        RelationType::OneToMany | RelationType::ManyToMany => ErEndpointKind::Many,
    }
}

fn er_endpoint_svg(
    x: usize,
    y: usize,
    side: ErEndpointSide,
    kind: ErEndpointKind,
    relation: &TableRelation,
    relation_key: usize,
    points_right: bool,
) -> String {
    let direction = if points_right { 1 } else { -1 };
    let endpoint = match side {
        ErEndpointSide::Source => "source",
        ErEndpointSide::Target => "target",
    };
    let class = relation_css_class(&relation.source);
    let source = escape_html_attr(&relation.source_table);
    let target = escape_html_attr(&relation.target_table);
    match kind {
        ErEndpointKind::One => format!(
                "        <g class=\"relation-marker {}\" data-relation-key=\"{}\" data-endpoint=\"{}\" data-endpoint-kind=\"one\" data-source=\"{}\" data-target=\"{}\" transform=\"translate({} {}) scale({} 1)\"><line x1=\"10\" y1=\"-10\" x2=\"10\" y2=\"10\" /></g>\n",
                class,
                relation_key,
                endpoint,
                source,
                target,
                x,
                y,
                direction
            ),
        ErEndpointKind::Many => format!(
                "        <g class=\"relation-marker {}\" data-relation-key=\"{}\" data-endpoint=\"{}\" data-endpoint-kind=\"many\" data-source=\"{}\" data-target=\"{}\" transform=\"translate({} {}) scale({} 1)\"><line x1=\"4\" y1=\"0\" x2=\"20\" y2=\"0\" /><line x1=\"4\" y1=\"0\" x2=\"20\" y2=\"-11\" /><line x1=\"4\" y1=\"0\" x2=\"20\" y2=\"11\" /></g>\n",
                class,
                relation_key,
                endpoint,
                source,
                target,
                x,
                y,
                direction
            ),
    }
}

fn markdown_cell(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        value.replace('|', "\\|").replace('\n', " ")
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_html_attr(value: &str) -> String {
    escape_html(value).replace('\'', "&#39;")
}

fn open_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let command = ("open", path.as_os_str());
    #[cfg(target_os = "windows")]
    let command = ("explorer", path.as_os_str());
    #[cfg(all(unix, not(target_os = "macos")))]
    let command = ("xdg-open", path.as_os_str());

    Command::new(command.0)
        .arg(command.1)
        .spawn()
        .map_err(|error| format!("Failed to open output directory: {error}"))?;
    Ok(())
}

fn validate_config(config: &AppConfig) -> Result<(), String> {
    require_text(&config.database.driver, "driver")?;
    require_text(&config.database.url, "url")?;
    require_text(&config.database.username, "username")?;
    require_text(&config.database.password, "password")?;
    require_text(&config.output.dir, "output.dir")?;
    require_text(&config.output.file_type, "file-type")?;
    if config.schemas.iter().all(|schema| schema.trim().is_empty()) {
        return Err("At least one schema is required.".to_string());
    }
    Ok(())
}

fn require_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} is required."))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn renders_fixture_documents_for_all_supported_file_types() {
        let output_dir = temp_output_dir();
        fs::create_dir_all(&output_dir).expect("create fixture output dir");
        let schema = fixture_schema();

        let html_path = render_schema(&schema, &engine("HTML"), output_dir.to_str().unwrap())
            .expect("render html fixture");
        let html = fs::read_to_string(&html_path).expect("read html fixture");
        assert!(html.contains("数据库设计文档"));
        assert!(html.contains("表清单"));
        assert!(html.contains("<code>users</code>"));
        assert!(html.contains("User display name"));

        let md_path = render_schema(&schema, &engine("MD"), output_dir.to_str().unwrap())
            .expect("render markdown fixture");
        let markdown = fs::read_to_string(&md_path).expect("read markdown fixture");
        assert!(markdown.contains("# 数据库设计文档"));
        assert!(markdown.contains("| 序号 | 表名 | 说明 |"));
        assert!(markdown.contains("| 2 | `display_name` | `varchar(120)` | Y | N |"));
        assert!(markdown.contains("idx_users_email"));

        let docx_path = render_schema(&schema, &engine("WORD"), output_dir.to_str().unwrap())
            .expect("render docx fixture");
        let docx = fs::read(&docx_path).expect("read docx fixture");
        assert!(docx.len() > 100);
        assert_eq!(&docx[0..2], b"PK");

        fs::remove_dir_all(&output_dir).expect("remove fixture output dir");
    }

    #[test]
    fn renders_english_labels_when_requested() {
        let output_dir = temp_output_dir();
        fs::create_dir_all(&output_dir).expect("create fixture output dir");
        let schema = fixture_schema();

        let markdown_path = render_schema(
            &schema,
            &engine_with_language("MD", "en-US"),
            output_dir.to_str().unwrap(),
        )
        .expect("render english markdown fixture");
        let markdown = fs::read_to_string(&markdown_path).expect("read markdown fixture");
        assert!(markdown.contains("# Database Dictionary"));
        assert!(markdown.contains("| No. | Table | Description |"));
        assert!(markdown.contains("| 1 | `id` | `bigint` | NO | YES |"));

        fs::remove_dir_all(&output_dir).expect("remove fixture output dir");
    }

    #[test]
    fn renders_er_diagram_html_with_table_nodes_and_relation_styles() {
        let output_dir = temp_output_dir();
        fs::create_dir_all(&output_dir).expect("create fixture output dir");
        let mut schema = relation_fixture_schema();
        schema.relations = merge_relations(
            Vec::new(),
            vec![TableRelation {
                id: "uploaded_orders_created_by_users_id".to_string(),
                name: None,
                source_schema: Some("fixture_schema".to_string()),
                source_table: "orders".to_string(),
                source_column: "created_by".to_string(),
                target_schema: Some("fixture_schema".to_string()),
                target_table: "users".to_string(),
                target_column: "id".to_string(),
                relation_type: RelationType::ManyToOne,
                source: RelationSource::Uploaded,
                confidence: 0.95,
                description: "创建人".to_string(),
            }],
            infer_relations_from_tables(&schema.name, &schema.tables, &empty_aliases()),
        );

        let er_path = render_er_diagram(&schema, output_dir.to_str().unwrap())
            .expect("render er diagram fixture");
        let html = fs::read_to_string(&er_path).expect("read er fixture");

        assert_eq!(er_path.file_name().unwrap(), "fixture_schema-er.html");
        assert!(html.contains("ER Diagram - fixture_schema"));
        assert!(html.contains("class=\"table-node\""));
        assert!(html.contains("data-zoom=\"in\""));
        assert!(html.contains("data-zoom=\"out\""));
        assert!(html.contains("data-diagram-content"));
        assert!(html.contains("orders"));
        assert!(html.contains("users"));
        assert!(html.contains("class=\"relation-line relation-inferred\""));
        assert!(html.contains("class=\"relation-marker relation-inferred\""));
        assert!(html.contains("class=\"relation-line relation-uploaded\""));
        assert!(html.contains("class=\"relation-marker relation-uploaded\""));
        assert!(html.contains("data-source-column=\"created_by\""));
        assert!(html.contains("data-target-column=\"id\""));
        assert!(html.contains("data-source-column=\"user_id\""));
        assert!(html.contains("class=\"column-row\" data-column=\"user_id\""));
        assert!(html.contains("function layoutRelations()"));
        assert!(html.contains("class=\"diagram-header\""));
        assert!(html.contains("flex: 1 1 auto"));
        assert!(!html.contains("关系清单"));
        assert!(!html.contains("弱关系推断示例"));

        fs::remove_dir_all(&output_dir).expect("remove fixture output dir");
    }

    #[test]
    fn er_diagram_layout_keeps_stacked_nodes_from_overlapping() {
        let schema = dense_er_fixture_schema();

        let positions = er_node_positions(&schema.tables, &[]);
        let first = positions
            .get("first_wide_table")
            .expect("first table position");
        let fourth = positions
            .get("fourth_table")
            .expect("fourth table position");

        assert!(fourth.y >= first.y + first.height + ER_NODE_GAP_Y);
    }

    #[test]
    fn er_diagram_layout_expands_columns_for_large_schemas() {
        assert_eq!(er_grid_columns(3), 3);
        assert_eq!(er_grid_columns(45), 3);
        assert_eq!(er_grid_columns(110), 8);
    }

    #[test]
    fn er_diagram_layout_keeps_related_tables_near_each_other() {
        let tables = vec![
            Table {
                name: "unrelated_a".to_string(),
                comment: String::new(),
                columns: vec![column("id", "bigint", false, "PRI")],
                indexes: Vec::new(),
            },
            Table {
                name: "archive_element_attachment".to_string(),
                comment: String::new(),
                columns: vec![column("element_id", "bigint", false, "MUL")],
                indexes: Vec::new(),
            },
            Table {
                name: "unrelated_b".to_string(),
                comment: String::new(),
                columns: vec![column("id", "bigint", false, "PRI")],
                indexes: Vec::new(),
            },
            Table {
                name: "archive_element".to_string(),
                comment: String::new(),
                columns: vec![column("element_id", "bigint", false, "PRI")],
                indexes: Vec::new(),
            },
        ];
        let relations = vec![TableRelation {
            id: "inferred_archive_element_attachment".to_string(),
            name: None,
            source_schema: Some("fixture_schema".to_string()),
            source_table: "archive_element".to_string(),
            source_column: "element_id".to_string(),
            target_schema: Some("fixture_schema".to_string()),
            target_table: "archive_element_attachment".to_string(),
            target_column: "element_id".to_string(),
            relation_type: RelationType::OneToMany,
            source: RelationSource::Inferred,
            confidence: 0.9,
            description: String::new(),
        }];

        let positions = er_node_positions(&tables, &relations);
        let parent = positions
            .get("archive_element")
            .expect("parent table position");
        let child = positions
            .get("archive_element_attachment")
            .expect("child table position");

        assert!(parent.y.abs_diff(child.y) <= ER_NODE_MIN_HEIGHT + ER_NODE_GAP_Y);
    }

    #[test]
    fn resolves_supported_database_inspectors_by_jdbc_url() {
        assert!(database_inspector(&data_source("jdbc:mysql://127.0.0.1:3306/app")).is_ok());
        assert!(database_inspector(&data_source("jdbc:postgresql://127.0.0.1:5432/app")).is_ok());
        assert!(
            database_inspector(&data_source("jdbc:oracle:thin:@//127.0.0.1:1521/XEPDB1")).is_ok()
        );
    }

    #[test]
    fn rejects_unknown_database_urls() {
        let error = match database_inspector(&data_source("jdbc:sqlserver://127.0.0.1:1433")) {
            Ok(_) => panic!("unknown database should be rejected"),
            Err(error) => error,
        };
        assert!(error.contains("Unsupported database URL"));
    }

    #[test]
    fn infers_high_confidence_id_relations_from_table_and_primary_key_names() {
        let schema = relation_fixture_schema();

        let aliases = [("dept".to_string(), "department".to_string())]
            .into_iter()
            .collect();
        let relations = infer_relations_from_tables(&schema.name, &schema.tables, &aliases);

        assert_eq!(relations.len(), 2);
        assert!(relations.iter().any(|relation| {
            relation.source_table == "orders"
                && relation.source_column == "user_id"
                && relation.target_table == "users"
                && relation.target_column == "id"
                && relation.source == RelationSource::Inferred
                && relation.confidence >= 0.8
        }));
        assert!(relations.iter().any(|relation| {
            relation.source_table == "orders"
                && relation.source_column == "dept_code"
                && relation.target_table == "departments"
                && relation.target_column == "code"
                && relation.source == RelationSource::Inferred
                && relation.confidence >= 0.8
        }));
    }

    #[test]
    fn infers_extension_table_relations_from_same_primary_business_key() {
        let schema = DatabaseSchema {
            name: "fixture_schema".to_string(),
            tables: vec![
                Table {
                    name: "ecm_archive_element_type".to_string(),
                    comment: "档案元素类型".to_string(),
                    columns: vec![
                        column("element_type_id", "bigint", false, "PRI"),
                        column("element_code", "varchar(30)", false, ""),
                    ],
                    indexes: vec![Index {
                        name: "PRIMARY".to_string(),
                        columns: vec!["element_type_id".to_string()],
                        unique: true,
                    }],
                },
                Table {
                    name: "ecm_archive_element_type_tl".to_string(),
                    comment: "档案元素类型多语言".to_string(),
                    columns: vec![
                        column("element_type_id", "bigint", false, "PRI"),
                        column("lang", "varchar(10)", false, "PRI"),
                        column("name", "varchar(120)", true, ""),
                    ],
                    indexes: vec![Index {
                        name: "PRIMARY".to_string(),
                        columns: vec!["element_type_id".to_string(), "lang".to_string()],
                        unique: true,
                    }],
                },
                Table {
                    name: "ecm_archive_element_dtl".to_string(),
                    comment: "档案元素明细".to_string(),
                    columns: vec![
                        column("element_id", "bigint", false, "PRI"),
                        column("element_type_id", "bigint", false, "MUL"),
                        column("element_code", "varchar(30)", false, ""),
                    ],
                    indexes: vec![
                        Index {
                            name: "PRIMARY".to_string(),
                            columns: vec!["element_id".to_string()],
                            unique: true,
                        },
                        Index {
                            name: "idx_element_dtl_type_id".to_string(),
                            columns: vec!["element_type_id".to_string()],
                            unique: false,
                        },
                    ],
                },
            ],
            relations: Vec::new(),
        };

        let relations = infer_relations_from_tables(&schema.name, &schema.tables, &empty_aliases());

        assert_eq!(relations.len(), 2);
        assert!(relations.iter().any(|relation| {
            relation.source_table == "ecm_archive_element_type"
                && relation.source_column == "element_type_id"
                && relation.target_table == "ecm_archive_element_type_tl"
                && relation.target_column == "element_type_id"
                && relation.relation_type == RelationType::OneToMany
                && relation.source == RelationSource::Inferred
                && relation.confidence >= 0.88
        }));
        assert!(relations.iter().any(|relation| {
            relation.source_table == "ecm_archive_element_type"
                && relation.source_column == "element_type_id"
                && relation.target_table == "ecm_archive_element_dtl"
                && relation.target_column == "element_type_id"
                && relation.relation_type == RelationType::OneToMany
                && relation.source == RelationSource::Inferred
                && relation.confidence >= 0.88
        }));
    }

    #[test]
    fn discovers_interface_archive_element_and_attachment_relation_chains() {
        let schema = DatabaseSchema {
            name: "fixture_schema".to_string(),
            tables: vec![
                Table {
                    name: "ecm_interface".to_string(),
                    comment: "接口".to_string(),
                    columns: vec![
                        column("interface_id", "bigint", false, "PRI"),
                        column("interface_code", "varchar(60)", false, ""),
                    ],
                    indexes: vec![Index {
                        name: "PRIMARY".to_string(),
                        columns: vec!["interface_id".to_string()],
                        unique: true,
                    }],
                },
                Table {
                    name: "ecm_interface_param".to_string(),
                    comment: "接口参数".to_string(),
                    columns: vec![
                        column("param_id", "bigint", false, "PRI"),
                        column("interface_id", "bigint", false, "MUL"),
                        column("param_name", "varchar(120)", false, ""),
                    ],
                    indexes: vec![
                        Index {
                            name: "PRIMARY".to_string(),
                            columns: vec!["param_id".to_string()],
                            unique: true,
                        },
                        Index {
                            name: "idx_interface_param_interface_id".to_string(),
                            columns: vec!["interface_id".to_string()],
                            unique: false,
                        },
                    ],
                },
                Table {
                    name: "ecm_archive".to_string(),
                    comment: "档案".to_string(),
                    columns: vec![
                        column("archive_id", "bigint", false, "PRI"),
                        column("archive_no", "varchar(60)", false, ""),
                    ],
                    indexes: vec![Index {
                        name: "PRIMARY".to_string(),
                        columns: vec!["archive_id".to_string()],
                        unique: true,
                    }],
                },
                Table {
                    name: "ecm_archive_dtl".to_string(),
                    comment: "档案明细".to_string(),
                    columns: vec![
                        column("archive_dtl_id", "bigint", false, "PRI"),
                        column("archive_id", "bigint", false, "MUL"),
                        column("element_id", "bigint", false, "MUL"),
                    ],
                    indexes: vec![
                        Index {
                            name: "PRIMARY".to_string(),
                            columns: vec!["archive_dtl_id".to_string()],
                            unique: true,
                        },
                        Index {
                            name: "idx_archive_dtl_archive_id".to_string(),
                            columns: vec!["archive_id".to_string()],
                            unique: false,
                        },
                        Index {
                            name: "idx_archive_dtl_element_id".to_string(),
                            columns: vec!["element_id".to_string()],
                            unique: false,
                        },
                    ],
                },
                Table {
                    name: "ecm_archive_element".to_string(),
                    comment: "档案元素".to_string(),
                    columns: vec![
                        column("element_id", "bigint", false, "PRI"),
                        column("element_code", "varchar(60)", false, ""),
                    ],
                    indexes: vec![Index {
                        name: "PRIMARY".to_string(),
                        columns: vec!["element_id".to_string()],
                        unique: true,
                    }],
                },
                Table {
                    name: "ecm_archive_element_attachment".to_string(),
                    comment: "元素附件".to_string(),
                    columns: vec![
                        column("attachment_id", "bigint", false, "PRI"),
                        column("element_id", "bigint", false, "MUL"),
                    ],
                    indexes: vec![
                        Index {
                            name: "PRIMARY".to_string(),
                            columns: vec!["attachment_id".to_string()],
                            unique: true,
                        },
                        Index {
                            name: "idx_element_attachment_element_id".to_string(),
                            columns: vec!["element_id".to_string()],
                            unique: false,
                        },
                    ],
                },
            ],
            relations: Vec::new(),
        };

        let relations = infer_relations_from_tables(&schema.name, &schema.tables, &empty_aliases());

        assert!(relations.iter().any(|relation| {
            relation.source_table == "ecm_interface"
                && relation.source_column == "interface_id"
                && relation.target_table == "ecm_interface_param"
                && relation.target_column == "interface_id"
                && relation.relation_type == RelationType::OneToMany
        }));
        assert!(relations.iter().any(|relation| {
            relation.source_table == "ecm_archive"
                && relation.source_column == "archive_id"
                && relation.target_table == "ecm_archive_dtl"
                && relation.target_column == "archive_id"
                && relation.relation_type == RelationType::OneToMany
        }));
        assert!(relations.iter().any(|relation| {
            relation.source_table == "ecm_archive_dtl"
                && relation.source_column == "element_id"
                && relation.target_table == "ecm_archive_element"
                && relation.target_column == "element_id"
                && relation.relation_type == RelationType::ManyToOne
        }));
        assert!(relations.iter().any(|relation| {
            relation.source_table == "ecm_archive_element"
                && relation.source_column == "element_id"
                && relation.target_table == "ecm_archive_element_attachment"
                && relation.target_column == "element_id"
                && relation.relation_type == RelationType::OneToMany
        }));
    }

    #[test]
    fn discovers_abbreviated_interface_parameter_one_to_one_relations() {
        let schema = DatabaseSchema {
            name: "fixture_schema".to_string(),
            tables: vec![
                interface_table("ecm_arch_elem_sync_itf"),
                interface_param_table("ecm_arc_elem_sync_itf_param"),
                interface_table("ecm_archive_element_itf"),
                interface_param_table("ecm_arc_elem_asyn_itf_param"),
            ],
            relations: Vec::new(),
        };

        let relations =
            infer_relations_from_tables(&schema.name, &schema.tables, &ecm_relation_aliases());

        assert!(relations.iter().any(|relation| {
            relation.source_table == "ecm_arch_elem_sync_itf"
                && relation.source_column == "interface_id"
                && relation.target_table == "ecm_arc_elem_sync_itf_param"
                && relation.target_column == "interface_id"
                && relation.relation_type == RelationType::OneToOne
                && relation.source == RelationSource::Inferred
        }));
        assert!(relations.iter().any(|relation| {
            relation.source_table == "ecm_archive_element_itf"
                && relation.source_column == "interface_id"
                && relation.target_table == "ecm_arc_elem_asyn_itf_param"
                && relation.target_column == "interface_id"
                && relation.relation_type == RelationType::OneToOne
                && relation.source == RelationSource::Inferred
        }));
    }

    #[test]
    fn applies_ignored_table_patterns_to_tables_and_relations() {
        let mut schema = relation_fixture_schema();
        schema.tables.push(Table {
            name: "audit_log".to_string(),
            comment: "Audit log".to_string(),
            columns: vec![column("id", "bigint", false, "PRI")],
            indexes: Vec::new(),
        });
        schema.relations =
            infer_relations_from_tables(&schema.name, &schema.tables, &empty_aliases());
        let config = AppConfig {
            database: data_source("jdbc:mysql://127.0.0.1:3306/fixture_schema"),
            schemas: vec!["fixture_schema".to_string()],
            tables: Some(crate::TablesConfig {
                ignore: Some(vec!["users".to_string(), "*_log".to_string()]),
            }),
            output: engine("HTML"),
            relations: None,
        };

        apply_table_filters(&mut schema, &config);

        assert!(schema.tables.iter().any(|table| table.name == "orders"));
        assert!(!schema.tables.iter().any(|table| table.name == "users"));
        assert!(!schema.tables.iter().any(|table| table.name == "audit_log"));
        assert!(schema.relations.iter().all(|relation| {
            relation.source_table != "users"
                && relation.target_table != "users"
                && relation.source_table != "audit_log"
                && relation.target_table != "audit_log"
        }));
    }

    #[test]
    fn reads_relation_aliases_from_config_for_discovery() {
        let mut aliases = HashMap::new();
        aliases.insert(
            "archive".to_string(),
            vec!["arc".to_string(), "arch".to_string()],
        );
        aliases.insert("element".to_string(), vec!["elem".to_string()]);
        let config = AppConfig {
            database: data_source("jdbc:mysql://127.0.0.1:3306/fixture_schema"),
            schemas: vec!["fixture_schema".to_string()],
            tables: None,
            output: engine("HTML"),
            relations: Some(crate::RelationsConfig {
                file: None,
                aliases: Some(aliases),
            }),
        };

        let aliases = relation_aliases(&config);

        assert_eq!(aliases.get("arc").map(String::as_str), Some("archive"));
        assert_eq!(aliases.get("arch").map(String::as_str), Some("archive"));
        assert_eq!(aliases.get("elem").map(String::as_str), Some("element"));
    }

    #[test]
    fn merge_relations_prefers_database_foreign_keys_over_inferred_duplicates() {
        let database_fk = TableRelation {
            id: "fk_orders_user_id_users_id".to_string(),
            name: Some("fk_orders_user".to_string()),
            source_schema: Some("fixture_schema".to_string()),
            source_table: "orders".to_string(),
            source_column: "user_id".to_string(),
            target_schema: Some("fixture_schema".to_string()),
            target_table: "users".to_string(),
            target_column: "id".to_string(),
            relation_type: RelationType::ManyToOne,
            source: RelationSource::DatabaseFk,
            confidence: 1.0,
            description: String::new(),
        };
        let inferred = TableRelation {
            id: "inferred_orders_user_id_users_id".to_string(),
            source: RelationSource::Inferred,
            confidence: 0.84,
            ..database_fk.clone()
        };

        let merged = merge_relations(vec![database_fk], Vec::new(), vec![inferred]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].source, RelationSource::DatabaseFk);
        assert_eq!(merged[0].confidence, 1.0);
    }

    #[test]
    fn merge_relations_keeps_external_relations_as_first_class_metadata() {
        let external = TableRelation {
            id: "uploaded_orders_created_by_users_id".to_string(),
            name: None,
            source_schema: None,
            source_table: "orders".to_string(),
            source_column: "created_by".to_string(),
            target_schema: None,
            target_table: "users".to_string(),
            target_column: "id".to_string(),
            relation_type: RelationType::ManyToOne,
            source: RelationSource::Uploaded,
            confidence: 0.95,
            description: "创建人".to_string(),
        };

        let merged = merge_relations(Vec::new(), vec![external], Vec::new());

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].source, RelationSource::Uploaded);
        assert_eq!(merged[0].source_column, "created_by");
    }

    #[test]
    fn reads_external_relation_json_files_for_future_upload_workflow() {
        let output_dir = temp_output_dir();
        fs::create_dir_all(&output_dir).expect("create fixture output dir");
        let relation_path = output_dir.join("relations.json");
        fs::write(
            &relation_path,
            r#"[
              {
                "sourceTable": "orders",
                "sourceColumn": "created_by",
                "targetTable": "users",
                "targetColumn": "id",
                "relationType": "many-to-one",
                "source": "uploaded",
                "description": "创建人"
              }
            ]"#,
        )
        .expect("write external relation fixture");

        let relations = read_external_relations(
            Some(relation_path.to_str().expect("relation path")),
            "fixture_schema",
        )
        .expect("read external relations");

        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].source, RelationSource::Uploaded);
        assert_eq!(
            relations[0].source_schema.as_deref(),
            Some("fixture_schema")
        );
        assert_eq!(relations[0].confidence, 0.95);
        assert_eq!(relations[0].description, "创建人");

        fs::remove_dir_all(&output_dir).expect("remove fixture output dir");
    }

    #[test]
    fn exports_inferred_relations_as_editable_relation_template_items() {
        let mut schema = relation_fixture_schema();
        let aliases = [("dept".to_string(), "department".to_string())]
            .into_iter()
            .collect();
        schema.relations = infer_relations_from_tables(&schema.name, &schema.tables, &aliases);

        let items = relation_template_items(&schema);

        assert!(items.iter().any(|item| {
            item.source_table == "orders"
                && item.source_column == "user_id"
                && item.target_table == "users"
                && item.target_column == "id"
                && item.relation_type == "many-to-one"
                && item.source == "uploaded"
                && item.confidence.is_some()
                && item._comment.contains("弱关系推断")
        }));
    }

    fn engine(file_type: &str) -> OutputConfig {
        engine_with_language(file_type, "zh-CN")
    }

    fn engine_with_language(file_type: &str, language: &str) -> OutputConfig {
        OutputConfig {
            dir: String::new(),
            open_dir: false,
            file_type: file_type.to_string(),
            language: Some(language.to_string()),
            file_name: Some("fixture-dictionary".to_string()),
        }
    }

    fn data_source(url: &str) -> DataSourceConfig {
        DataSourceConfig {
            driver: "mysql".to_string(),
            url: url.to_string(),
            username: "user".to_string(),
            password: "password".to_string(),
        }
    }

    fn empty_aliases() -> RelationAliases {
        RelationAliases::new()
    }

    fn ecm_relation_aliases() -> RelationAliases {
        [
            ("arc", "archive"),
            ("arch", "archive"),
            ("elem", "element"),
            ("itf", "interface"),
            ("asyn", "async"),
        ]
        .into_iter()
        .map(|(alias, canonical)| (alias.to_string(), canonical.to_string()))
        .collect()
    }

    fn interface_table(name: &str) -> Table {
        Table {
            name: name.to_string(),
            comment: "接口".to_string(),
            columns: vec![
                column("interface_id", "bigint", false, "PRI"),
                column("interface_code", "varchar(60)", false, ""),
            ],
            indexes: vec![Index {
                name: "PRIMARY".to_string(),
                columns: vec!["interface_id".to_string()],
                unique: true,
            }],
        }
    }

    fn interface_param_table(name: &str) -> Table {
        Table {
            name: name.to_string(),
            comment: "接口参数".to_string(),
            columns: vec![
                column("param_id", "bigint", false, "PRI"),
                column("interface_id", "bigint", false, "UNI"),
                column("param_value", "varchar(120)", true, ""),
            ],
            indexes: vec![
                Index {
                    name: "PRIMARY".to_string(),
                    columns: vec!["param_id".to_string()],
                    unique: true,
                },
                Index {
                    name: format!("uk_{name}_interface_id"),
                    columns: vec!["interface_id".to_string()],
                    unique: true,
                },
            ],
        }
    }

    fn fixture_schema() -> DatabaseSchema {
        DatabaseSchema {
            name: "fixture_schema".to_string(),
            tables: vec![Table {
                name: "users".to_string(),
                comment: "Application users".to_string(),
                columns: vec![
                    Column {
                        name: "id".to_string(),
                        data_type: "bigint".to_string(),
                        nullable: false,
                        default_value: String::new(),
                        comment: "Primary key".to_string(),
                        key: "PRI".to_string(),
                        extra: "auto_increment".to_string(),
                    },
                    Column {
                        name: "display_name".to_string(),
                        data_type: "varchar(120)".to_string(),
                        nullable: true,
                        default_value: String::new(),
                        comment: "User display name".to_string(),
                        key: String::new(),
                        extra: String::new(),
                    },
                    Column {
                        name: "email".to_string(),
                        data_type: "varchar(255)".to_string(),
                        nullable: false,
                        default_value: String::new(),
                        comment: "Login email".to_string(),
                        key: "UNI".to_string(),
                        extra: String::new(),
                    },
                ],
                indexes: vec![
                    Index {
                        name: "PRIMARY".to_string(),
                        columns: vec!["id".to_string()],
                        unique: true,
                    },
                    Index {
                        name: "idx_users_email".to_string(),
                        columns: vec!["email".to_string()],
                        unique: true,
                    },
                ],
            }],
            relations: Vec::new(),
        }
    }

    fn relation_fixture_schema() -> DatabaseSchema {
        DatabaseSchema {
            name: "fixture_schema".to_string(),
            tables: vec![
                Table {
                    name: "users".to_string(),
                    comment: "Application users".to_string(),
                    columns: vec![
                        column("id", "bigint", false, "PRI"),
                        column("email", "varchar(255)", false, "UNI"),
                    ],
                    indexes: vec![Index {
                        name: "PRIMARY".to_string(),
                        columns: vec!["id".to_string()],
                        unique: true,
                    }],
                },
                Table {
                    name: "departments".to_string(),
                    comment: "Departments".to_string(),
                    columns: vec![
                        column("id", "bigint", false, "PRI"),
                        column("code", "varchar(32)", false, "UNI"),
                    ],
                    indexes: vec![
                        Index {
                            name: "PRIMARY".to_string(),
                            columns: vec!["id".to_string()],
                            unique: true,
                        },
                        Index {
                            name: "uk_departments_code".to_string(),
                            columns: vec!["code".to_string()],
                            unique: true,
                        },
                    ],
                },
                Table {
                    name: "orders".to_string(),
                    comment: "Orders".to_string(),
                    columns: vec![
                        column("id", "bigint", false, "PRI"),
                        column("user_id", "bigint", false, "MUL"),
                        column("dept_code", "varchar(32)", true, "MUL"),
                        column("note", "varchar(255)", true, ""),
                    ],
                    indexes: vec![
                        Index {
                            name: "PRIMARY".to_string(),
                            columns: vec!["id".to_string()],
                            unique: true,
                        },
                        Index {
                            name: "idx_orders_user_id".to_string(),
                            columns: vec!["user_id".to_string()],
                            unique: false,
                        },
                        Index {
                            name: "idx_orders_dept_code".to_string(),
                            columns: vec!["dept_code".to_string()],
                            unique: false,
                        },
                    ],
                },
            ],
            relations: Vec::new(),
        }
    }

    fn dense_er_fixture_schema() -> DatabaseSchema {
        DatabaseSchema {
            name: "dense_schema".to_string(),
            tables: vec![
                Table {
                    name: "first_wide_table".to_string(),
                    comment: String::new(),
                    columns: (0..18)
                        .map(|index| column(&format!("column_{index}"), "varchar(50)", true, ""))
                        .collect(),
                    indexes: Vec::new(),
                },
                Table {
                    name: "second_table".to_string(),
                    comment: String::new(),
                    columns: vec![column("id", "bigint", false, "PRI")],
                    indexes: Vec::new(),
                },
                Table {
                    name: "third_table".to_string(),
                    comment: String::new(),
                    columns: vec![column("id", "bigint", false, "PRI")],
                    indexes: Vec::new(),
                },
                Table {
                    name: "fourth_table".to_string(),
                    comment: String::new(),
                    columns: vec![column("id", "bigint", false, "PRI")],
                    indexes: Vec::new(),
                },
            ],
            relations: Vec::new(),
        }
    }

    fn column(name: &str, data_type: &str, nullable: bool, key: &str) -> Column {
        Column {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable,
            default_value: String::new(),
            comment: String::new(),
            key: key.to_string(),
            extra: String::new(),
        }
    }

    fn temp_output_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "schema-forge-fixture-{}-{nanos}",
            std::process::id()
        ))
    }
}

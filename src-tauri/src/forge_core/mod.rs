use crate::{AppConfig, DataSourceConfig, EngineConfig};
use docx_rs::{
    Docx, Paragraph, Run, Shading, ShdType, Table as DocxTable, TableCell, TableLayoutType,
    TableRow, WidthType,
};
use mysql::{params, prelude::Queryable, OptsBuilder, Pool};
use serde::Deserialize;
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

trait DatabaseInspector {
    fn inspect_schema(&self, schema: &str) -> Result<DatabaseSchema, String>;
}

#[derive(Debug)]
struct DatabaseSchema {
    name: String,
    tables: Vec<Table>,
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

impl ForgeCore {
    pub fn generate(config: &AppConfig) -> Result<GenerateOutput, String> {
        validate_config(config)?;
        let schemas = config
            .screw
            .schemas
            .iter()
            .map(|schema| schema.trim())
            .filter(|schema| !schema.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let output_dir = config.screw.engine.file_output_dir.trim().to_string();
        fs::create_dir_all(&output_dir)
            .map_err(|error| format!("Failed to create output directory: {error}"))?;

        let inspector = database_inspector(&config.spring.datasource)?;
        let mut generated_files = Vec::new();
        for schema in &schemas {
            let database = inspector.inspect_schema(schema)?;
            let path = render_schema(&database, &config.screw.engine, &output_dir)?;
            generated_files.push(path.display().to_string());
        }

        if config.screw.engine.open_output_dir {
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

        Ok(DatabaseSchema {
            name: schema.to_string(),
            tables,
        })
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
    engine: &EngineConfig,
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
    fn from_engine(engine: &EngineConfig) -> Result<Self, String> {
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

fn render_docx(database: &DatabaseSchema, labels: Labels) -> Docx {
    let mut doc = Docx::new()
        .add_paragraph(title_heading(&labels.doc_title))
        .add_table(meta_docx_table(database, &labels))
        .add_paragraph(section_heading(&labels.table_directory))
        .add_table(directory_docx_table(database, &labels));
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
            .add_table(column_docx_table(table, &labels));
        if !table.indexes.is_empty() {
            doc = doc
                .add_paragraph(subsection_heading(&labels.indexes))
                .add_table(index_docx_table(table, &labels));
        }
    }
    doc
}

fn meta_docx_table(database: &DatabaseSchema, labels: &Labels) -> DocxTable {
    styled_docx_table(vec![
        docx_row(
            vec![labels.database_name.to_string(), database.name.clone()],
            false,
        ),
        docx_row(
            vec![labels.document_version.to_string(), "1.0.0".to_string()],
            false,
        ),
        docx_row(
            vec![
                labels.document_description.to_string(),
                "Database design document".to_string(),
            ],
            false,
        ),
    ])
}

fn directory_docx_table(database: &DatabaseSchema, labels: &Labels) -> DocxTable {
    let mut rows = vec![docx_row(
        vec![
            labels.sequence.to_string(),
            labels.table_name.to_string(),
            labels.description.to_string(),
        ],
        true,
    )];
    for (index, table) in database.tables.iter().enumerate() {
        rows.push(docx_row(
            vec![
                (index + 1).to_string(),
                table.name.clone(),
                table.comment.clone(),
            ],
            false,
        ));
    }
    styled_docx_table(rows)
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
        ));
    }
    styled_docx_table(rows)
}

fn index_docx_table(table: &Table, labels: &Labels) -> DocxTable {
    let mut rows = vec![docx_row(
        vec![
            labels.index_name.to_string(),
            labels.unique.to_string(),
            labels.columns.to_string(),
        ],
        true,
    )];
    for index in &table.indexes {
        rows.push(docx_row(
            vec![
                index.name.clone(),
                labels.bool(index.unique).to_string(),
                index.columns.join(", "),
            ],
            false,
        ));
    }
    styled_docx_table(rows)
}

fn styled_docx_table(rows: Vec<TableRow>) -> DocxTable {
    DocxTable::new(rows)
        .width(5000, WidthType::Pct)
        .layout(TableLayoutType::Autofit)
}

fn docx_row(values: Vec<String>, header: bool) -> TableRow {
    TableRow::new(
        values
            .iter()
            .map(|value| docx_cell(value, header))
            .collect(),
    )
}

fn docx_cell(value: &str, header: bool) -> TableCell {
    let run = if header {
        Run::new()
            .add_text(value)
            .bold()
            .size(20)
            .color(DOC_PRIMARY_COLOR)
    } else {
        Run::new().add_text(value).size(19).color(DOC_TEXT_COLOR)
    };
    let cell = TableCell::new().add_paragraph(Paragraph::new().add_run(run));
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

fn title_heading(value: &str) -> Paragraph {
    Paragraph::new().add_run(
        Run::new()
            .add_text(value)
            .bold()
            .size(32)
            .color(DOC_PRIMARY_COLOR),
    )
}

fn section_heading(value: &str) -> Paragraph {
    Paragraph::new().add_run(
        Run::new()
            .add_text(value)
            .bold()
            .size(24)
            .color(DOC_PRIMARY_COLOR),
    )
}

fn subsection_heading(value: &str) -> Paragraph {
    Paragraph::new().add_run(
        Run::new()
            .add_text(value)
            .bold()
            .size(20)
            .color(DOC_TEXT_COLOR),
    )
}

fn paragraph(value: &str) -> Paragraph {
    Paragraph::new().add_run(Run::new().add_text(value).size(20).color(DOC_MUTED_COLOR))
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
    require_text(
        &config.spring.datasource.driver_class_name,
        "driver-class-name",
    )?;
    require_text(&config.spring.datasource.url, "url")?;
    require_text(&config.spring.datasource.username, "username")?;
    require_text(&config.spring.datasource.password, "password")?;
    require_text(&config.screw.engine.file_output_dir, "file-output-dir")?;
    require_text(&config.screw.engine.file_type, "file-type")?;
    if config
        .screw
        .schemas
        .iter()
        .all(|schema| schema.trim().is_empty())
    {
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

    fn engine(file_type: &str) -> EngineConfig {
        engine_with_language(file_type, "zh-CN")
    }

    fn engine_with_language(file_type: &str, language: &str) -> EngineConfig {
        EngineConfig {
            file_output_dir: String::new(),
            open_output_dir: false,
            file_type: file_type.to_string(),
            produce_type: "forgecore".to_string(),
            language: Some(language.to_string()),
            file_name: Some("fixture-dictionary".to_string()),
        }
    }

    fn data_source(url: &str) -> DataSourceConfig {
        DataSourceConfig {
            driver_class_name: String::new(),
            url: url.to_string(),
            username: "user".to_string(),
            password: "password".to_string(),
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

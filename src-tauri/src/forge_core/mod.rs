use crate::{AppConfig, DataSourceConfig, EngineConfig};
use docx_rs::{Docx, Paragraph, Run, Table as DocxTable, TableCell, TableRow};
use mysql::{params, prelude::Queryable, OptsBuilder, Pool};
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

        let inspector = MySqlInspector::new(&config.spring.datasource)?;
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

struct MySqlInspector {
    source: DataSourceConfig,
}

impl MySqlInspector {
    fn new(source: &DataSourceConfig) -> Result<Self, String> {
        if !source.url.trim().starts_with("jdbc:mysql://") {
            return Err("ForgeCore currently supports MySQL JDBC URLs only.".to_string());
        }
        Ok(Self {
            source: source.clone(),
        })
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
    let base_name = engine
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&database.name);
    match file_type.as_str() {
        "HTML" => write_file(output_dir, base_name, "html", &render_html(database)),
        "MD" => write_file(output_dir, base_name, "md", &render_markdown(database)),
        "WORD" => write_docx(output_dir, base_name, database),
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
) -> Result<PathBuf, String> {
    let path = Path::new(output_dir).join(format!("{}.docx", safe_file_name(base_name)));
    let file = File::create(&path)
        .map_err(|error| format!("Failed to create {}: {error}", path.display()))?;
    render_docx(database)
        .build()
        .pack(file)
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
    Ok(path)
}

fn render_markdown(database: &DatabaseSchema) -> String {
    let mut md = format!("# Database Dictionary: {}\n\n", database.name);
    for table in &database.tables {
        md.push_str(&format!("## `{}`\n\n", table.name));
        if !table.comment.is_empty() {
            md.push_str(&format!("{}\n\n", table.comment));
        }
        md.push_str("| Column | Type | Nullable | Key | Default | Extra | Comment |\n");
        md.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
        for column in &table.columns {
            md.push_str(&format!(
                "| `{}` | `{}` | {} | {} | {} | {} | {} |\n",
                column.name,
                column.data_type,
                yes_no(column.nullable),
                markdown_cell(&column.key),
                markdown_cell(&column.default_value),
                markdown_cell(&column.extra),
                markdown_cell(&column.comment)
            ));
        }
        if !table.indexes.is_empty() {
            md.push_str("\n**Indexes**\n\n");
            md.push_str("| Name | Unique | Columns |\n");
            md.push_str("| --- | --- | --- |\n");
            for index in &table.indexes {
                md.push_str(&format!(
                    "| `{}` | {} | {} |\n",
                    index.name,
                    yes_no(index.unique),
                    index.columns.join(", ")
                ));
            }
        }
        md.push('\n');
    }
    md
}

fn render_html(database: &DatabaseSchema) -> String {
    let mut html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Database Dictionary - {}</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; color: #17201c; margin: 32px; }}
    h1 {{ margin-bottom: 24px; }}
    h2 {{ border-bottom: 1px solid #d7dfda; padding-bottom: 8px; margin-top: 32px; }}
    table {{ border-collapse: collapse; width: 100%; margin: 12px 0 24px; }}
    th, td {{ border: 1px solid #d7dfda; padding: 8px 10px; text-align: left; vertical-align: top; }}
    th {{ background: #edf4ef; }}
    code {{ background: #edf4ef; border-radius: 4px; padding: 1px 4px; }}
    .muted {{ color: #68756d; }}
  </style>
</head>
<body>
  <h1>Database Dictionary: {}</h1>
"#,
        escape_html(&database.name),
        escape_html(&database.name)
    );
    for table in &database.tables {
        html.push_str(&format!(
            "<h2><code>{}</code></h2>\n",
            escape_html(&table.name)
        ));
        if !table.comment.is_empty() {
            html.push_str(&format!("<p>{}</p>\n", escape_html(&table.comment)));
        }
        html.push_str("<table><thead><tr><th>Column</th><th>Type</th><th>Nullable</th><th>Key</th><th>Default</th><th>Extra</th><th>Comment</th></tr></thead><tbody>\n");
        for column in &table.columns {
            html.push_str(&format!(
                "<tr><td><code>{}</code></td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                escape_html(&column.name),
                escape_html(&column.data_type),
                yes_no(column.nullable),
                escape_html(&column.key),
                escape_html(&column.default_value),
                escape_html(&column.extra),
                escape_html(&column.comment)
            ));
        }
        html.push_str("</tbody></table>\n");
        if !table.indexes.is_empty() {
            html.push_str("<h3>Indexes</h3><table><thead><tr><th>Name</th><th>Unique</th><th>Columns</th></tr></thead><tbody>\n");
            for index in &table.indexes {
                html.push_str(&format!(
                    "<tr><td><code>{}</code></td><td>{}</td><td>{}</td></tr>\n",
                    escape_html(&index.name),
                    yes_no(index.unique),
                    escape_html(&index.columns.join(", "))
                ));
            }
            html.push_str("</tbody></table>\n");
        }
    }
    html.push_str("</body>\n</html>\n");
    html
}

fn render_docx(database: &DatabaseSchema) -> Docx {
    let mut doc = Docx::new().add_paragraph(heading(
        &format!("Database Dictionary: {}", database.name),
        32,
    ));
    for table in &database.tables {
        doc = doc.add_paragraph(heading(&table.name, 24));
        if !table.comment.is_empty() {
            doc = doc.add_paragraph(paragraph(&table.comment));
        }
        doc = doc.add_table(column_docx_table(table));
        if !table.indexes.is_empty() {
            doc = doc
                .add_paragraph(heading("Indexes", 20))
                .add_table(index_docx_table(table));
        }
    }
    doc
}

fn column_docx_table(table: &Table) -> DocxTable {
    let mut rows = vec![docx_row(&[
        "Column", "Type", "Nullable", "Key", "Default", "Extra", "Comment",
    ])];
    for column in &table.columns {
        rows.push(docx_row(&[
            &column.name,
            &column.data_type,
            yes_no(column.nullable),
            &column.key,
            &column.default_value,
            &column.extra,
            &column.comment,
        ]));
    }
    DocxTable::new(rows)
}

fn index_docx_table(table: &Table) -> DocxTable {
    let mut rows = vec![docx_row(&["Name", "Unique", "Columns"])];
    for index in &table.indexes {
        rows.push(docx_row(&[
            &index.name,
            yes_no(index.unique),
            &index.columns.join(", "),
        ]));
    }
    DocxTable::new(rows)
}

fn docx_row(values: &[&str]) -> TableRow {
    TableRow::new(values.iter().map(|value| docx_cell(value)).collect())
}

fn docx_cell(value: &str) -> TableCell {
    TableCell::new().add_paragraph(paragraph(value))
}

fn heading(value: &str, size: usize) -> Paragraph {
    Paragraph::new().add_run(Run::new().add_text(value).bold().size(size))
}

fn paragraph(value: &str) -> Paragraph {
    Paragraph::new().add_run(Run::new().add_text(value))
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

fn yes_no(value: bool) -> &'static str {
    if value {
        "YES"
    } else {
        "NO"
    }
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
        assert!(html.contains("Database Dictionary: fixture_schema"));
        assert!(html.contains("<code>users</code>"));
        assert!(html.contains("User display name"));

        let md_path = render_schema(&schema, &engine("MD"), output_dir.to_str().unwrap())
            .expect("render markdown fixture");
        let markdown = fs::read_to_string(&md_path).expect("read markdown fixture");
        assert!(markdown.contains("# Database Dictionary: fixture_schema"));
        assert!(markdown.contains("| `display_name` | `varchar(120)` | YES |"));
        assert!(markdown.contains("idx_users_email"));

        let docx_path = render_schema(&schema, &engine("WORD"), output_dir.to_str().unwrap())
            .expect("render docx fixture");
        let docx = fs::read(&docx_path).expect("read docx fixture");
        assert!(docx.len() > 100);
        assert_eq!(&docx[0..2], b"PK");

        fs::remove_dir_all(&output_dir).expect("remove fixture output dir");
    }

    fn engine(file_type: &str) -> EngineConfig {
        EngineConfig {
            file_output_dir: String::new(),
            open_output_dir: false,
            file_type: file_type.to_string(),
            produce_type: "forgecore".to_string(),
            file_name: Some("fixture-dictionary".to_string()),
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

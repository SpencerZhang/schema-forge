mod forge_core;

use forge_core::ForgeCore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct AppConfig {
    database: DataSourceConfig,
    schemas: Vec<String>,
    tables: Option<TablesConfig>,
    output: OutputConfig,
    relations: Option<RelationsConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DataSourceConfig {
    driver: String,
    url: String,
    username: String,
    password: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct OutputConfig {
    dir: String,
    #[serde(rename = "open-dir")]
    open_dir: bool,
    #[serde(rename = "file-type")]
    file_type: String,
    #[serde(rename = "language")]
    language: Option<String>,
    #[serde(rename = "file-name")]
    file_name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TablesConfig {
    ignore: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct RelationsConfig {
    file: Option<String>,
    aliases: Option<std::collections::HashMap<String, Vec<String>>>,
}

#[derive(Debug, Serialize)]
struct GenerateResponse {
    schemas: Vec<String>,
    output_dir: String,
    stdout: String,
}

#[derive(Debug, Serialize)]
struct RelationTemplateResponse {
    file_name: String,
    content: String,
    relation_count: usize,
}

#[tauri::command]
fn generate_doc(config: AppConfig) -> Result<GenerateResponse, String> {
    let result = ForgeCore::generate(&config)?;
    Ok(GenerateResponse {
        schemas: result.schemas,
        output_dir: result.output_dir,
        stdout: result.stdout,
    })
}

#[tauri::command]
fn generate_er_diagram(config: AppConfig) -> Result<GenerateResponse, String> {
    let result = ForgeCore::generate_er_diagram(&config)?;
    Ok(GenerateResponse {
        schemas: result.schemas,
        output_dir: result.output_dir,
        stdout: result.stdout,
    })
}

#[tauri::command]
fn generate_relation_template(config: AppConfig) -> Result<RelationTemplateResponse, String> {
    let result = ForgeCore::generate_relation_template(&config)?;
    Ok(RelationTemplateResponse {
        file_name: result.file_name,
        content: result.content,
        relation_count: result.relation_count,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            generate_doc,
            generate_er_diagram,
            generate_relation_template
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

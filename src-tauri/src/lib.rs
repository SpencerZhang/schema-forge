mod forge_core;

use forge_core::ForgeCore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct AppConfig {
    spring: SpringConfig,
    screw: ScrewConfig,
}

#[derive(Debug, Deserialize, Serialize)]
struct SpringConfig {
    datasource: DataSourceConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DataSourceConfig {
    #[serde(rename = "driver-class-name")]
    driver_class_name: String,
    url: String,
    username: String,
    password: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ScrewConfig {
    schemas: Vec<String>,
    engine: EngineConfig,
}

#[derive(Debug, Deserialize, Serialize)]
struct EngineConfig {
    #[serde(rename = "file-output-dir")]
    file_output_dir: String,
    #[serde(rename = "open-output-dir")]
    open_output_dir: bool,
    #[serde(rename = "file-type")]
    file_type: String,
    #[serde(rename = "produce-type")]
    produce_type: String,
    #[serde(rename = "language")]
    language: Option<String>,
    #[serde(rename = "file-name")]
    file_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct GenerateResponse {
    schemas: Vec<String>,
    output_dir: String,
    stdout: String,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![generate_doc])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

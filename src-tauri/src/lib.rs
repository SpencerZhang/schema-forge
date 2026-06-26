use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

#[derive(Debug, Deserialize, Serialize)]
struct AppConfig {
    spring: SpringConfig,
    screw: ScrewConfig,
}

#[derive(Debug, Deserialize, Serialize)]
struct SpringConfig {
    datasource: DataSourceConfig,
}

#[derive(Debug, Deserialize, Serialize)]
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
fn generate_doc(app: tauri::AppHandle, config: AppConfig) -> Result<GenerateResponse, String> {
    validate_config(&config)?;
    let config_path = write_temp_config(&config)?;
    let jar_path = generator_jar_path(&app)?;
    let output = Command::new("java")
        .arg("-jar")
        .arg(&jar_path)
        .arg("--config")
        .arg(&config_path)
        .output()
        .map_err(|error| format!("Failed to start Java generator: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let _ = fs::remove_file(&config_path);

    if !output.status.success() {
        return Err(if stderr.trim().is_empty() {
            "Java generator failed.".to_string()
        } else {
            stderr
        });
    }

    Ok(GenerateResponse {
        schemas: config.screw.schemas,
        output_dir: config.screw.engine.file_output_dir,
        stdout,
    })
}

fn validate_config(config: &AppConfig) -> Result<(), String> {
    require_text(&config.spring.datasource.driver_class_name, "driver-class-name")?;
    require_text(&config.spring.datasource.url, "url")?;
    require_text(&config.spring.datasource.username, "username")?;
    require_text(&config.spring.datasource.password, "password")?;
    require_text(&config.screw.engine.file_output_dir, "file-output-dir")?;
    require_text(&config.screw.engine.file_type, "file-type")?;
    require_text(&config.screw.engine.produce_type, "produce-type")?;
    if config.screw.schemas.iter().all(|schema| schema.trim().is_empty()) {
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

fn write_temp_config(config: &AppConfig) -> Result<PathBuf, String> {
    let mut path = std::env::temp_dir();
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("Failed to read system time: {error}"))?
        .as_millis();
    path.push(format!("schema-forge-{millis}.yml"));
    let yaml = serde_yaml::to_string(config)
        .map_err(|error| format!("Failed to serialize temp config: {error}"))?;
    fs::write(&path, yaml).map_err(|error| format!("Failed to write temp config: {error}"))?;
    Ok(path)
}

fn generator_jar_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled_jar = resource_dir.join("schema-forge-generator.jar");
        if bundled_jar.exists() {
            return Ok(bundled_jar);
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_dir = manifest_dir
        .parent()
        .ok_or_else(|| "Failed to resolve project directory.".to_string())?;
    let jar_path = project_dir
        .join("backend")
        .join("target")
        .join("schema-forge-generator-0.1.0.jar");
    if Path::new(&jar_path).exists() {
        Ok(jar_path)
    } else {
        Err(format!(
            "Generator jar not found: {}. Run `npm run generator:build` first.",
            jar_path.display()
        ))
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![generate_doc])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

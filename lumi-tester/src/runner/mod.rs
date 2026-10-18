pub mod context;
pub mod events;
pub mod executor;
pub mod healer;
pub mod js_engine;
pub mod shell;
pub mod state;

use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use events::*;
pub use healer::*;
pub use state::*;

/// Construct the existing platform driver for a canonical App component.
/// Multi-target orchestration uses this helper opportunistically; if the local
/// device environment is unavailable the caller can preserve a structured
/// BLOCKED report instead of replacing the existing App driver.
pub async fn create_platform_driver_for_target(
    platform: &str,
    devices: &[String],
    speed: Option<&str>,
) -> Result<Box<dyn crate::driver::traits::PlatformDriver>> {
    let platform = platform
        .trim_matches('"')
        .trim_matches('\'')
        .to_ascii_lowercase();
    let device = devices.first().map(String::as_str);
    match platform.as_str() {
        "android" => Ok(Box::new(
            crate::driver::android::AndroidDriver::new_with_speed(device, speed).await?,
        )),
        "android_auto" => Ok(Box::new(
            crate::driver::android_auto::AndroidAutoDriver::new(device, true).await?,
        )),
        "ios" => Ok(Box::new(crate::driver::ios::IosDriver::new(device).await?)),
        "web" => Ok(Box::new(
            crate::driver::web::WebDriver::new(crate::driver::web::WebDriverConfig::default())
                .await?,
        )),
        "macos" => Ok(Box::new(crate::driver::macos::MacosDriver::new())),
        "windows" => Ok(Box::new(crate::driver::windows::WindowsDriver::new())),
        _ => anyhow::bail!("Unknown App platform: {platform}"),
    }
}

/// Run tests from a file or directory
pub async fn run_tests(
    path: &Path,
    platform: &str,
    devices: Option<Vec<String>>,
    output: &Path,
    continue_on_failure: bool,
    parallel: bool,
    record: bool,
    snapshot: bool,
    report: bool,
    events_jsonl: bool,
    tags: Option<Vec<String>>,
    command_index: Option<usize>,
    from_command_index: Option<usize>,
    command_name: Option<String>,
    repeat: u32,
    data: Option<&Path>,
) -> Result<()> {
    let platform = platform
        .trim_matches('"')
        .trim_matches('\'')
        .to_ascii_lowercase();

    // 1. Resolve devices
    let device_serials = match devices {
        Some(d) => d,
        None => {
            if platform == "android" || platform == "android_auto" {
                let connected = crate::driver::android::adb::get_devices().await?;
                if connected.is_empty() {
                    anyhow::bail!("No Android devices connected");
                }
                connected.into_iter().map(|d| d.serial).collect()
            } else if platform == "web" {
                vec!["chromium".to_string()]
            } else if platform == "macos" || platform == "windows" {
                vec!["local".to_string()]
            } else {
                vec!["".to_string()] // Default for others
            }
        }
    };

    if device_serials.is_empty() {
        anyhow::bail!("No devices available for execution");
    }

    // 2. Collect all test files
    let mut all_files = Vec::new();
    if path.is_dir() {
        for entry in walkdir::WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path = e.path();
                let is_yaml = path
                    .extension()
                    .map_or(false, |ext| ext == "yaml" || ext == "yml");
                let name = e.file_name().to_string_lossy();

                // Skip files in utility directories that are meant to be called by scenario flows.
                let path_str = path.to_string_lossy();
                let in_subflows =
                    path_str.contains("/subflows/") || path_str.contains("\\subflows\\");
                let in_screens =
                    path_str.contains("/screens/") || path_str.contains("\\screens\\");

                is_yaml
                    && !in_subflows
                    && !in_screens
                    && name != "setup.yaml"
                    && name != "setup.yml"
                    && name != "teardown.yaml"
                    && name != "teardown.yml"
            })
        {
            all_files.push(entry.path().to_path_buf());
        }
    } else {
        all_files.push(path.to_path_buf());
    }

    if all_files.is_empty() {
        println!("{} No test files found.", "ℹ".blue());
        return Ok(());
    }

    // 3. Execution logic
    if parallel && device_serials.len() > 1 {
        println!(
            "{} Parallel execution enabled across {} devices",
            "🚀".yellow(),
            device_serials.len()
        );

        let chunk_size = (all_files.len() as f64 / device_serials.len() as f64).ceil() as usize;
        let chunks = all_files.chunks(chunk_size);

        let mut handles = Vec::new();
        let path_owned = path.to_path_buf();
        let platform_owned = platform.clone();
        let output_owned = Some(output.to_path_buf());

        for (i, chunk) in chunks.enumerate() {
            let device = device_serials[i].clone();
            let files = chunk.to_vec();

            // Auto-detect platform from device ID if possible
            let mut device_platform = platform_owned.clone();
            if device.contains('-') && device.len() == 36 {
                // Heuristic: UUID format usually implies iOS simulator/device
                device_platform = "ios".to_string();
            } else if device.contains('.') || device.chars().all(|c| c.is_alphanumeric()) {
                // IP address or alphanumeric serial usually implies Android
                // But check if it conflicts with iOS heuristic?
                // iOS UUID is alphanumeric + dashes. Android serial is alphanum.
                // We'll stick to: if it LOOKS like a UUID, it's iOS. Else default to provided platform or Android.
                if platform_owned == "auto" {
                    device_platform = "android".to_string();
                }
            }

            let output = output_owned.clone();
            let base_path = path_owned.clone();
            let tags_chunk = tags.clone();
            let cmd_idx = command_index;
            let from_cmd_idx = from_command_index;
            let cmd_name = command_name.clone();
            let data_owned = data.map(|p| p.to_path_buf());

            let handle = tokio::spawn(async move {
                run_on_device(
                    &base_path,
                    &files,
                    &device_platform,
                    Some(&device),
                    output.as_deref(),
                    continue_on_failure,
                    record,
                    snapshot,
                    report,
                    events_jsonl,
                    tags_chunk,
                    cmd_idx,
                    from_cmd_idx,
                    cmd_name,
                    repeat,
                    data_owned.as_deref(),
                )
                .await
            });
            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await?;
        }

        println!("{} All parallel test tasks finished.", "✅".green());
        Ok(())
    } else {
        // Sequential run on primary device (or all files on one device)
        let primary_device = device_serials.first().map(|s| s.as_str());
        run_on_device(
            path,
            &all_files,
            &platform,
            primary_device,
            Some(output),
            continue_on_failure,
            record,
            snapshot,
            report,
            events_jsonl,
            tags,
            command_index,
            from_command_index,
            command_name,
            repeat,
            data,
        )
        .await
    }
}

/// Run a set of files on a specific device
async fn run_on_device(
    base_path: &Path,
    files: &[PathBuf],
    platform: &str,
    device: Option<&str>,
    output: Option<&Path>,
    continue_on_failure: bool,
    record: bool,
    snapshot: bool,
    report: bool,
    events_jsonl: bool,
    tags: Option<Vec<String>>,
    command_index: Option<usize>,
    from_command_index: Option<usize>,
    command_name: Option<String>,
    repeat: u32,
    data: Option<&Path>,
) -> Result<()> {
    // Pre-parse first file's `speed:` header field (e.g. "fast", "turbo") so the driver
    // can honor a flow-level speed profile when LUMI_SPEED isn't set.
    let flow_speed: Option<String> = if !files.is_empty() {
        crate::parser::yaml::parse_test_file(&files[0])
            .ok()
            .and_then(|flow| flow.speed)
    } else {
        None
    };

    // Pre-parse first file to extract web driver config (for close_when_finish support)
    let web_config = if platform == "web" && !files.is_empty() {
        use crate::parser::yaml::parse_test_file;

        // Parse first file to get header config
        if let Ok(flow) = parse_test_file(&files[0]) {
            use crate::driver::web::{BrowserType, WebDriverConfig};
            let mut config = WebDriverConfig::default();

            // Apply close_when_finish from YAML header
            if let Some(close) = flow.close_when_finish {
                config.close_when_finish = close;
            }

            // Apply browser type if specified
            if let Some(ref b) = flow.browser {
                config.browser_type = match b.to_lowercase().as_str() {
                    "firefox" => BrowserType::Firefox,
                    "webkit" => BrowserType::Webkit,
                    _ => BrowserType::Chromium,
                };
            }
            Some(config)
        } else {
            None
        }
    } else {
        None
    };

    // Strip quotes from platform if present (YAML parsing quirk)
    let platform_clean = platform
        .trim_matches('"')
        .trim_matches('\'')
        .to_ascii_lowercase();

    let driver: Box<dyn crate::driver::traits::PlatformDriver> = match platform_clean.as_str() {
        "android" => Box::new(
            crate::driver::android::AndroidDriver::new_with_speed(
                device,
                flow_speed.as_deref(),
            )
            .await?,
        ),
        "android_auto" => {
            Box::new(crate::driver::android_auto::AndroidAutoDriver::new(device, true).await?)
        }
        "web" => {
            use crate::driver::web::{WebDriver, WebDriverConfig};
            let config = web_config.unwrap_or_else(WebDriverConfig::default);
            Box::new(WebDriver::new(config).await?)
        }
        "ios" => Box::new(crate::driver::ios::IosDriver::new(device).await?),
        "macos" => Box::new(crate::driver::macos::MacosDriver::new()),
        "windows" => Box::new(crate::driver::windows::WindowsDriver::new()),
        _ => anyhow::bail!("Unknown platform: {}", platform_clean),
    };

    let target_name = if base_path.is_file() {
        base_path.file_stem().map(|s| s.to_string_lossy().to_string())
    } else {
        base_path.file_name().map(|s| s.to_string_lossy().to_string())
    };

    let mut executor = executor::TestExecutor::new_with_events(
        driver,
        output,
        continue_on_failure,
        record,
        snapshot,
        report,
        tags,
        events_jsonl,
    )
    .with_target_name(target_name.as_deref());
    let base_dir = if base_path.is_dir() {
        base_path
    } else {
        base_path.parent().unwrap_or(Path::new("."))
    };

    // 1. Run Setup hook
    for f in ["setup.yaml", "setup.yml"] {
        let p = base_dir.join(f);
        if p.exists() {
            if let Err(e) = executor.run_file(&p, None, None, None).await {
                let _ = executor.finish().await;
                return Err(e);
            }
            break;
        }
    }

    // 2. Run Main files (repeated N times or per data row)
    let data_records = if let Some(dp) = data {
        Some(load_data_records(dp)?)
    } else {
        None
    };

    if let Some(ref records) = data_records {
        for (data_idx, data_row) in records.iter().enumerate() {
            println!(
                "\n{} Data iteration {}/{} ({})",
                "📊".cyan().bold(),
                data_idx + 1,
                records.len(),
                data_row
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            for (k, v) in data_row {
                executor.context.vars.insert(k.clone(), v.clone());
            }
            for file in files {
                if let Err(e) = executor
                    .run_file(file, command_index, from_command_index, command_name.as_deref())
                    .await
                {
                    eprintln!(
                        "  {} {} failed to run (data row {}): {} (continuing to next file - continue-on-failure)",
                        "⚠️".yellow(),
                        file.display(),
                        data_idx + 1,
                        e
                    );
                    if !continue_on_failure {
                        let _ = executor.finish().await;
                        return Err(e);
                    }
                }
            }
        }
    } else {
        for round in 0..repeat {
            if repeat > 1 {
                println!(
                    "\n{} Repeat round {}/{}",
                    "🔁".cyan(),
                    round + 1,
                    repeat
                );
            }
            for file in files {
                if let Err(e) = executor
                    .run_file(file, command_index, from_command_index, command_name.as_deref())
                    .await
                {
                    eprintln!(
                        "  {} {} failed to run: {} (continuing to next file - continue-on-failure)",
                        "⚠️".yellow(),
                        file.display(),
                        e
                    );
                    if !continue_on_failure {
                        let _ = executor.finish().await;
                        return Err(e);
                    }
                }
            }
        }
    }

    // 3. Run Teardown hook
    for f in ["teardown.yaml", "teardown.yml"] {
        let p = base_dir.join(f);
        if p.exists() {
            if let Err(e) = executor.run_file(&p, None, None, None).await {
                let _ = executor.finish().await;
                return Err(e);
            }
            break;
        }
    }

    executor.finish().await
}

/// Load records from a CSV or JSON data file for data-driven testing
pub fn load_data_records(data_path: &Path) -> Result<Vec<HashMap<String, String>>> {
    if !data_path.exists() {
        anyhow::bail!("Data file does not exist: {}", data_path.display());
    }
    let ext = data_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let mut records = Vec::new();
    if ext == "csv" {
        let mut rdr = csv::Reader::from_path(data_path)
            .with_context(|| format!("Failed to read CSV data file: {}", data_path.display()))?;
        let headers = rdr.headers()?.clone();
        for result in rdr.records() {
            let record = result?;
            let mut map = HashMap::new();
            for (i, field) in record.iter().enumerate() {
                if let Some(header) = headers.get(i) {
                    map.insert(header.trim().to_string(), field.trim().to_string());
                }
            }
            records.push(map);
        }
    } else if ext == "json" {
        let content = std::fs::read_to_string(data_path)
            .with_context(|| format!("Failed to read JSON data file: {}", data_path.display()))?;
        let json_val: serde_json::Value = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON data in {}", data_path.display()))?;
        if let serde_json::Value::Array(arr) = json_val {
            for item in arr {
                let mut map = HashMap::new();
                if let serde_json::Value::Object(obj) = item {
                    for (k, v) in obj {
                        let v_str = match v {
                            serde_json::Value::String(s) => s,
                            _ => v.to_string(),
                        };
                        map.insert(k, v_str);
                    }
                }
                records.push(map);
            }
        } else {
            anyhow::bail!("JSON data file must contain a JSON array of objects");
        }
    } else {
        anyhow::bail!(
            "Unsupported data file format (expected .csv or .json): {}",
            data_path.display()
        );
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_csv_data_records() {
        let temp_dir = std::env::temp_dir().join(format!(
            "lumi_test_csv_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&temp_dir);
        let csv_path = temp_dir.join("users.csv");
        std::fs::write(&csv_path, "username,role\nalice,admin\nbob,user\n").unwrap();

        let records = load_data_records(&csv_path).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].get("username").map(|s| s.as_str()), Some("alice"));
        assert_eq!(records[0].get("role").map(|s| s.as_str()), Some("admin"));
        assert_eq!(records[1].get("username").map(|s| s.as_str()), Some("bob"));
        assert_eq!(records[1].get("role").map(|s| s.as_str()), Some("user"));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_json_data_records() {
        let temp_dir = std::env::temp_dir().join(format!(
            "lumi_test_json_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&temp_dir);
        let json_path = temp_dir.join("users.json");
        std::fs::write(
            &json_path,
            r#"[{"username": "charlie", "count": 10}, {"username": "david", "count": 20}]"#,
        )
        .unwrap();

        let records = load_data_records(&json_path).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].get("username").map(|s| s.as_str()), Some("charlie"));
        assert_eq!(records[0].get("count").map(|s| s.as_str()), Some("10"));
        assert_eq!(records[1].get("username").map(|s| s.as_str()), Some("david"));
        assert_eq!(records[1].get("count").map(|s| s.as_str()), Some("20"));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}

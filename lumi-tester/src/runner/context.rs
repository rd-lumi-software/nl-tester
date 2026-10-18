use crate::parser::types::TestFlow;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

/// Test execution context that holds runtime information
#[derive(Clone)]
pub struct TestContext {
    /// Base directory for test files (for resolving relative paths)
    pub base_dir: std::path::PathBuf,

    /// Output directory for screenshots, recordings, reports
    pub output_dir: std::path::PathBuf,

    /// Current app ID being tested
    pub app_id: Option<String>,

    /// Current URL (for web testing)
    pub url: Option<String>,

    /// Environment variables for the test
    pub env: HashMap<String, String>,

    /// User-defined variables (set via setVar command)
    pub vars: HashMap<String, String>,

    /// Continue running tests even if one fails
    pub continue_on_failure: bool,

    /// Device ID (for isolating outputs in parallel runs)
    pub device_id: Option<String>,

    /// Default timeout for implicit waits
    pub default_timeout_ms: u64,
}

impl TestContext {
    pub fn new(
        base_dir: &Path,
        output_dir: Option<&Path>,
        continue_on_failure: bool,
        device_id: Option<String>,
    ) -> Self {
        let mut output = output_dir.map(|p| p.to_path_buf()).unwrap_or_else(|| {
            let mut path = base_dir.to_path_buf();
            path.push("output");
            path
        });

        if let Some(ref id) = device_id {
            output.push(id.replace(':', "_")); // Ensure valid directory name
        }

        // Always ensure output directory exists
        let _ = std::fs::create_dir_all(&output);

        Self {
            base_dir: base_dir.to_path_buf(),
            output_dir: output,
            app_id: None,
            url: None,
            env: HashMap::new(),
            vars: HashMap::new(),
            continue_on_failure,
            device_id,
            default_timeout_ms: 10000, // Default 10s
        }
    }

    /// Update context from a test flow's header
    pub fn update_from_flow(&mut self, flow: &TestFlow) {
        if let Some(ref app_id) = flow.app_id {
            self.app_id = Some(app_id.clone());
        }
        if let Some(ref url) = flow.url {
            self.url = Some(url.clone());
        }
        if let Some(ref env) = flow.env {
            for (k, v) in env {
                self.env.insert(k.clone(), v.clone());
            }
        }
        if let Some(ref vars) = flow.vars {
            for (k, v) in vars {
                self.set_var(k, v);
            }
        }
        if let Some(timeout) = flow.default_timeout_ms {
            self.default_timeout_ms = timeout;
        }
    }

    /// Resolve a relative path to an absolute path
    pub fn resolve_path(&self, relative: &str) -> std::path::PathBuf {
        let path = std::path::Path::new(relative);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base_dir.join(path)
        }
    }

    /// Get the output path for a file
    pub fn output_path(&self, filename: &str) -> std::path::PathBuf {
        self.output_dir.join(filename)
    }

    /// Get a variable from env or vars
    pub fn get_var(&self, name: &str) -> Option<String> {
        self.vars
            .get(name)
            .cloned()
            .or_else(|| self.env.get(name).cloned())
            .or_else(|| std::env::var(name).ok())
    }

    /// Set a variable
    pub fn set_var(&mut self, name: &str, value: &str) {
        // Substitute any ${varname} in the value
        let substituted = self.substitute_vars(value);
        self.vars.insert(name.to_string(), substituted);
    }

    /// Substitute ${varname}, ${varname:-default}, or ${varname.json.path} patterns in a string
    pub fn substitute_vars(&self, text: &str) -> String {
        // Regex to match ${key} or ${key:-default} where key can contain dots
        let re = Regex::new(r"\$\{([a-zA-Z0-9_.]+)(?::-([^}]*))?\}").unwrap();
        let result = re
            .replace_all(text, |caps: &regex::Captures| {
                let full_key = &caps[1];
                let default_val = caps.get(2).map(|m| m.as_str());

                // 1. Try explicit full match first
                if let Some(val) = self.get_var(full_key) {
                    if !val.is_empty() || default_val.is_none() {
                        return val;
                    }
                }

                // 1b. Handle dynamic time variables
                match full_key {
                    "time" => return chrono::Local::now().format("%H:%M:%S").to_string(),
                    "date" => return chrono::Local::now().format("%Y-%m-%d").to_string(),
                    "timestamp" => return chrono::Utc::now().timestamp().to_string(),
                    _ => {}
                }

                // 2. Try splitting by first dot to access JSON object
                if full_key.contains('.') {
                    let parts: Vec<&str> = full_key.splitn(2, '.').collect();
                    if parts.len() == 2 {
                        let var_name = parts[0];
                        let json_path = parts[1];

                        if let Some(json_str) = self.get_var(var_name) {
                            // Try to parse variable content as JSON
                            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json_str)
                            {
                                // JSON pointer requires / separator instead of .
                                let pointer = format!("/{}", json_path.replace('.', "/"));

                                if let Some(target) = value.pointer(&pointer) {
                                    // Return string representation
                                    if let Some(s) = target.as_str() {
                                        return s.to_string();
                                    }
                                    return target.to_string();
                                }
                            }
                        }
                    }
                }

                // 3. Fallback to default if provided
                if let Some(def) = default_val {
                    return self.substitute_vars(def);
                }

                // 4. Keep original if not found and no default provided
                caps[0].to_string()
            })
            .to_string();

        result
    }

    /// Merge variables from a nested flow call
    pub fn merge_vars(&mut self, vars: &HashMap<String, String>) {
        for (k, v) in vars {
            let substituted = self.substitute_vars(v);
            self.vars.insert(k.clone(), substituted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_vars_basic_and_default() {
        let mut ctx = TestContext::new(Path::new("."), None, false, None);
        ctx.set_var("device", "Pixel 7");
        ctx.set_var("empty_var", "");

        assert_eq!(ctx.substitute_vars("Hello ${device}"), "Hello Pixel 7");
        assert_eq!(ctx.substitute_vars("Port: ${JIG_PORT:-COM5}"), "Port: COM5");
        assert_eq!(ctx.substitute_vars("Device: ${device:-fallback}"), "Device: Pixel 7");
        assert_eq!(ctx.substitute_vars("Empty: ${empty_var:-default_val}"), "Empty: default_val");
        assert_eq!(ctx.substitute_vars("Unset: ${UNSET_VAR}"), "Unset: ${UNSET_VAR}");
    }

    #[test]
    fn test_substitute_vars_json_pointer() {
        let mut ctx = TestContext::new(Path::new("."), None, false, None);
        ctx.vars.insert(
            "user".to_string(),
            r#"{"name": "Lumi", "role": "tester", "nested": {"id": 42}}"#.to_string(),
        );

        assert_eq!(ctx.substitute_vars("Name: ${user.name}"), "Name: Lumi");
        assert_eq!(ctx.substitute_vars("ID: ${user.nested.id}"), "ID: 42");
        assert_eq!(ctx.substitute_vars("Missing: ${user.missing:-guest}"), "Missing: guest");
    }
}

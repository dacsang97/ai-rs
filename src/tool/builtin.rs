use async_trait::async_trait;
use globset::GlobBuilder;
use ignore::WalkBuilder;
use schemars::schema_for;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::{ToolDef, ToolExecutor, ToolResult};

// ---------------------------------------------------------------------------
// Input schemas
// ---------------------------------------------------------------------------

#[derive(Deserialize, schemars::JsonSchema)]
struct BashInput {
    /// The shell command to execute
    command: String,
    /// Timeout in seconds (default: 30, max: 300)
    timeout_secs: Option<u64>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ReadFileInput {
    /// Path to the file or directory (relative to workspace or absolute within it)
    path: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct WriteFileInput {
    /// Path to the file (relative to workspace or absolute within it)
    path: String,
    /// The full content to write
    content: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct EditFileInput {
    /// Path to the file
    path: String,
    /// The exact string to find and replace
    old_str: String,
    /// The replacement string
    new_str: String,
    /// Replace all occurrences (default: false, replaces only the first)
    replace_all: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct GlobInput {
    /// Glob pattern (e.g., "**/*.ts")
    pattern: String,
    /// Subdirectory to search in (relative to workspace)
    path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct GrepInput {
    /// Regex pattern to search for
    pattern: String,
    /// Subdirectory or file to search in (relative to workspace)
    path: Option<String>,
    /// Glob pattern to filter which files are searched (e.g., "*.rs")
    include: Option<String>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_STDOUT_BYTES: usize = 50 * 1024;
const MAX_STDERR_BYTES: usize = 10 * 1024;
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 300;

const BLOCKED_PATTERNS: &[&str] = &[
    "sudo ",
    "rm -rf /",
    "chmod 777",
    "mkfs",
    "dd if=",
    "format c:",
];

const BLOCKED_PREFIXES: &[&str] = &["sudo "];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_and_validate(path: &str, workspace: &str) -> Result<PathBuf, String> {
    if path.contains("..") {
        return Err("Path traversal (..) is not allowed".to_string());
    }

    let ws = Path::new(workspace)
        .canonicalize()
        .map_err(|e| format!("Invalid workspace: {e}"))?;

    let target = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        ws.join(path)
    };

    if let Some(parent) = target.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create parent directories: {e}"))?;
        }
    }

    let resolved = if target.exists() {
        target
            .canonicalize()
            .map_err(|e| format!("Failed to resolve path: {e}"))?
    } else {
        let parent = target
            .parent()
            .ok_or_else(|| "Invalid path".to_string())?
            .canonicalize()
            .map_err(|e| format!("Failed to resolve parent path: {e}"))?;
        parent.join(
            target
                .file_name()
                .ok_or_else(|| "Invalid file name".to_string())?,
        )
    };

    if !resolved.starts_with(&ws) {
        return Err(format!(
            "Path '{}' is outside workspace '{}'",
            resolved.display(),
            ws.display()
        ));
    }

    Ok(resolved)
}

fn check_command_safety(command: &str) -> Result<(), String> {
    let lower = command.to_lowercase();

    for pattern in BLOCKED_PATTERNS {
        if lower.contains(pattern) {
            return Err(format!(
                "Blocked: command contains dangerous pattern '{pattern}'"
            ));
        }
    }

    let trimmed = lower.trim_start();
    for prefix in BLOCKED_PREFIXES {
        if trimmed.starts_with(prefix) {
            return Err(format!("Blocked: command starts with '{prefix}'"));
        }
    }

    Ok(())
}

fn truncate_output(s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }
    let truncated = &s[..max_bytes];
    let valid = match std::str::from_utf8(truncated.as_bytes()) {
        Ok(_) => truncated,
        Err(e) => &truncated[..e.valid_up_to()],
    };
    format!(
        "{}\n\n--- OUTPUT TRUNCATED (showing {}/{} bytes) ---",
        valid,
        valid.len(),
        s.len()
    )
}

fn tool_def(name: &str, description: &str, schema: serde_json::Value) -> ToolDef {
    ToolDef {
        name: name.to_string(),
        description: description.to_string(),
        input_schema: schema,
    }
}

// ---------------------------------------------------------------------------
// BuiltinTools
// ---------------------------------------------------------------------------

pub struct BuiltinTools {
    workspace: String,
}

impl BuiltinTools {
    pub fn new(workspace: String) -> Self {
        Self { workspace }
    }

    async fn exec_bash(&self, input: BashInput) -> Result<String, String> {
        check_command_safety(&input.command)?;

        let timeout_secs = input
            .timeout_secs
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(MAX_TIMEOUT_SECS);

        log::info!("tool:bash running command (timeout={timeout_secs}s)");

        let (shell, flag) = if cfg!(target_os = "windows") {
            ("powershell.exe", "-Command")
        } else {
            ("/bin/bash", "-c")
        };

        let mut cmd = tokio::process::Command::new(shell);
        cmd.arg(flag).arg(&input.command);
        cmd.current_dir(&self.workspace);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn process: {e}"))?;

        let timeout_dur = std::time::Duration::from_secs(timeout_secs);
        match tokio::time::timeout(timeout_dur, child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = truncate_output(
                    String::from_utf8_lossy(&output.stdout).to_string(),
                    MAX_STDOUT_BYTES,
                );
                let stderr = truncate_output(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                    MAX_STDERR_BYTES,
                );
                let code = output.status.code().unwrap_or(-1);

                let mut result = String::new();
                if !stdout.is_empty() {
                    result.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    result.push_str("[stderr]\n");
                    result.push_str(&stderr);
                }
                if code != 0 {
                    result.push_str(&format!("\n[exit code: {code}]"));
                }
                Ok(result)
            }
            Ok(Err(e)) => Err(format!("Process error: {e}")),
            Err(_) => Err(format!("Process timed out after {timeout_secs} seconds")),
        }
    }

    async fn exec_read_file(&self, input: ReadFileInput) -> Result<String, String> {
        let resolved = resolve_and_validate(&input.path, &self.workspace)?;

        log::info!("tool:read_file {}", resolved.display());

        if resolved.is_dir() {
            let mut entries = Vec::new();
            let mut dir = tokio::fs::read_dir(&resolved)
                .await
                .map_err(|e| format!("Failed to read directory '{}': {e}", resolved.display()))?;
            while let Some(entry) = dir
                .next_entry()
                .await
                .map_err(|e| format!("Failed to read entry: {e}"))?
            {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry
                    .file_type()
                    .await
                    .map(|t| t.is_dir())
                    .unwrap_or(false);
                entries.push(if is_dir {
                    format!("{name}/")
                } else {
                    name
                });
            }
            entries.sort();
            return Ok(entries.join("\n"));
        }

        let content = std::fs::read_to_string(&resolved)
            .map_err(|e| format!("Failed to read file '{}': {e}", resolved.display()))?;

        let numbered: String = content
            .lines()
            .enumerate()
            .map(|(i, line)| format!("{:>4}: {}", i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(numbered)
    }

    async fn exec_write_file(&self, input: WriteFileInput) -> Result<String, String> {
        let resolved = resolve_and_validate(&input.path, &self.workspace)?;

        log::info!("tool:write_file {}", resolved.display());

        std::fs::write(&resolved, &input.content)
            .map_err(|e| format!("Failed to write file '{}': {e}", resolved.display()))?;

        Ok(format!(
            "Successfully wrote {} bytes to {}",
            input.content.len(),
            resolved.display()
        ))
    }

    async fn exec_edit_file(&self, input: EditFileInput) -> Result<String, String> {
        let resolved = resolve_and_validate(&input.path, &self.workspace)?;

        log::info!("tool:edit_file {}", resolved.display());

        let content = std::fs::read_to_string(&resolved)
            .map_err(|e| format!("Failed to read file '{}': {e}", resolved.display()))?;

        if !content.contains(&input.old_str) {
            return Err(format!(
                "old_str not found in '{}'. No changes made.",
                resolved.display()
            ));
        }

        let new_content = if input.replace_all.unwrap_or(false) {
            content.replace(&input.old_str, &input.new_str)
        } else {
            content.replacen(&input.old_str, &input.new_str, 1)
        };

        std::fs::write(&resolved, &new_content)
            .map_err(|e| format!("Failed to write file '{}': {e}", resolved.display()))?;

        Ok(format!("Successfully edited {}", resolved.display()))
    }

    async fn exec_glob(&self, input: GlobInput) -> Result<String, String> {
        let root = match &input.path {
            Some(p) if !p.is_empty() => resolve_and_validate(p, &self.workspace)?,
            _ => Path::new(&self.workspace)
                .canonicalize()
                .map_err(|e| format!("Invalid workspace: {e}"))?,
        };

        if !root.exists() {
            return Err(format!("Path '{}' does not exist", root.display()));
        }

        let glob = GlobBuilder::new(&input.pattern)
            .literal_separator(false)
            .build()
            .map_err(|e| format!("Invalid glob pattern: {e}"))?
            .compile_matcher();

        let walker = WalkBuilder::new(&root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build();

        let mut results: Vec<(String, bool, u64, std::time::SystemTime)> = Vec::new();

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let entry_path = entry.path();
            if entry_path == root {
                continue;
            }

            let file_name = match entry_path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            let rel_path = entry_path
                .strip_prefix(&root)
                .unwrap_or(entry_path)
                .to_string_lossy();

            if !glob.is_match(file_name) && !glob.is_match(rel_path.as_ref()) {
                continue;
            }

            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            results.push((
                entry_path.to_string_lossy().to_string(),
                meta.is_dir(),
                meta.len(),
                mtime,
            ));
        }

        results.sort_by(|a, b| b.3.cmp(&a.3));
        results.truncate(100);

        let lines: Vec<String> = results
            .iter()
            .map(|(path, is_dir, size, _)| {
                if *is_dir {
                    format!("{path}/")
                } else {
                    format!("{path} ({size} bytes)")
                }
            })
            .collect();

        Ok(lines.join("\n"))
    }

    async fn exec_grep(&self, input: GrepInput) -> Result<String, String> {
        let root = match &input.path {
            Some(p) if !p.is_empty() => resolve_and_validate(p, &self.workspace)?,
            _ => Path::new(&self.workspace)
                .canonicalize()
                .map_err(|e| format!("Invalid workspace: {e}"))?,
        };

        if !root.exists() {
            return Err(format!("Path '{}' does not exist", root.display()));
        }

        let re = regex::Regex::new(&input.pattern)
            .map_err(|e| format!("Invalid regex pattern: {e}"))?;

        let include_glob = match &input.include {
            Some(g) if !g.is_empty() => Some(
                GlobBuilder::new(g)
                    .literal_separator(false)
                    .build()
                    .map_err(|e| format!("Invalid include glob: {e}"))?
                    .compile_matcher(),
            ),
            _ => None,
        };

        let walker = WalkBuilder::new(&root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build();

        let mut file_entries: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let entry_path = entry.path();
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !meta.is_file() {
                continue;
            }
            if let Some(ref ig) = include_glob {
                let fname = entry_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                let rel = entry_path
                    .strip_prefix(&root)
                    .unwrap_or(entry_path)
                    .to_string_lossy();
                if !ig.is_match(fname) && !ig.is_match(rel.as_ref()) {
                    continue;
                }
            }
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            file_entries.push((entry_path.to_path_buf(), mtime));
        }

        file_entries.sort_by(|a, b| b.1.cmp(&a.1));

        let mut output = String::new();
        let mut total_matches = 0usize;
        const MAX_TOTAL: usize = 100;
        const MAX_PER_FILE: usize = 10;
        const MAX_LINE_CHARS: usize = 2000;

        for (file_path, _) in &file_entries {
            if total_matches >= MAX_TOTAL {
                break;
            }

            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let mut file_matches: Vec<String> = Vec::new();

            for (idx, line) in content.lines().enumerate() {
                if file_matches.len() >= MAX_PER_FILE || total_matches >= MAX_TOTAL {
                    break;
                }
                if re.is_match(line) {
                    let truncated = if line.len() > MAX_LINE_CHARS {
                        format!("{}…", &line[..MAX_LINE_CHARS])
                    } else {
                        line.to_string()
                    };
                    file_matches.push(format!("  {}:{}", idx + 1, truncated));
                    total_matches += 1;
                }
            }

            if !file_matches.is_empty() {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&file_path.to_string_lossy());
                output.push('\n');
                output.push_str(&file_matches.join("\n"));
            }
        }

        if output.is_empty() {
            Ok("No matches found.".to_string())
        } else {
            Ok(output)
        }
    }
}

#[async_trait]
impl ToolExecutor for BuiltinTools {
    async fn execute(&self, name: &str, input: Value) -> crate::Result<ToolResult> {
        let result = match name {
            "bash" => {
                let params: BashInput = serde_json::from_value(input)
                    .map_err(|e| crate::AiError::Tool {
                        tool: name.to_string(),
                        message: format!("Invalid input: {e}"),
                    })?;
                self.exec_bash(params).await
            }
            "read_file" => {
                let params: ReadFileInput = serde_json::from_value(input)
                    .map_err(|e| crate::AiError::Tool {
                        tool: name.to_string(),
                        message: format!("Invalid input: {e}"),
                    })?;
                self.exec_read_file(params).await
            }
            "write_file" => {
                let params: WriteFileInput = serde_json::from_value(input)
                    .map_err(|e| crate::AiError::Tool {
                        tool: name.to_string(),
                        message: format!("Invalid input: {e}"),
                    })?;
                self.exec_write_file(params).await
            }
            "edit_file" => {
                let params: EditFileInput = serde_json::from_value(input)
                    .map_err(|e| crate::AiError::Tool {
                        tool: name.to_string(),
                        message: format!("Invalid input: {e}"),
                    })?;
                self.exec_edit_file(params).await
            }
            "glob" => {
                let params: GlobInput = serde_json::from_value(input)
                    .map_err(|e| crate::AiError::Tool {
                        tool: name.to_string(),
                        message: format!("Invalid input: {e}"),
                    })?;
                self.exec_glob(params).await
            }
            "grep" => {
                let params: GrepInput = serde_json::from_value(input)
                    .map_err(|e| crate::AiError::Tool {
                        tool: name.to_string(),
                        message: format!("Invalid input: {e}"),
                    })?;
                self.exec_grep(params).await
            }
            _ => {
                return Err(crate::AiError::Tool {
                    tool: name.to_string(),
                    message: format!("Unknown builtin tool '{name}'"),
                });
            }
        };

        match result {
            Ok(output) => Ok(ToolResult {
                output,
                title: None,
                is_error: false,
            }),
            Err(msg) => Ok(ToolResult {
                output: msg,
                title: None,
                is_error: true,
            }),
        }
    }

    fn definitions(&self) -> Vec<ToolDef> {
        vec![
            tool_def(
                "bash",
                "Execute a shell command in the workspace directory. Returns stdout, stderr, and exit code.",
                serde_json::to_value(schema_for!(BashInput)).unwrap_or_default(),
            ),
            tool_def(
                "read_file",
                "Read a file's contents with line numbers, or list a directory's entries. Path is relative to workspace or absolute within it.",
                serde_json::to_value(schema_for!(ReadFileInput)).unwrap_or_default(),
            ),
            tool_def(
                "write_file",
                "Create or overwrite a file with the given content. Parent directories are created automatically. Path is relative to workspace or absolute within it.",
                serde_json::to_value(schema_for!(WriteFileInput)).unwrap_or_default(),
            ),
            tool_def(
                "edit_file",
                "Find and replace text in an existing file. Replaces the first occurrence by default; set replace_all to true to replace all occurrences.",
                serde_json::to_value(schema_for!(EditFileInput)).unwrap_or_default(),
            ),
            tool_def(
                "glob",
                "Find files matching a glob pattern. Returns up to 100 results sorted by most recently modified. Respects .gitignore.",
                serde_json::to_value(schema_for!(GlobInput)).unwrap_or_default(),
            ),
            tool_def(
                "grep",
                "Search file contents for a regex pattern. Returns up to 100 matches (max 10 per file), sorted by most recently modified files. Respects .gitignore.",
                serde_json::to_value(schema_for!(GrepInput)).unwrap_or_default(),
            ),
        ]
    }
}

mod parser;

pub use parser::{SkillFrontmatter, parse_skill_md};

use std::path::{Path, PathBuf};

/// Metadata about a discovered skill.
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

/// Registry of skills discovered from workspace skill directories.
pub struct SkillRegistry {
    skills: Vec<SkillInfo>,
}

/// Directories (relative to workspace root) to scan for skills.
const SKILL_DIRS: &[&str] = &[".claude/skills", ".agents/skills", ".opencode/skills"];

impl SkillRegistry {
    /// Scan standard skill directories under `workspace` and build the registry.
    ///
    /// Each skill directory should contain a `SKILL.md` with YAML frontmatter.
    /// Missing directories are silently skipped.
    pub async fn discover(workspace: &Path) -> Self {
        let mut skills = Vec::new();

        for dir in SKILL_DIRS {
            let base = workspace.join(dir);
            let entries = match tokio::fs::read_dir(&base).await {
                Ok(entries) => entries,
                Err(_) => continue, // directory doesn't exist — skip
            };

            let mut entries = entries;
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let skill_file = path.join("SKILL.md");
                let content = match tokio::fs::read_to_string(&skill_file).await {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                if let Some((fm, _)) = parse_skill_md(&content) {
                    skills.push(SkillInfo {
                        name: fm.name,
                        description: fm.description,
                        path: skill_file,
                    });
                }
            }
        }

        Self { skills }
    }

    /// Return all discovered skills.
    pub fn list(&self) -> &[SkillInfo] {
        &self.skills
    }

    /// Load the full body content of a skill by name.
    pub async fn load(&self, name: &str) -> crate::Result<String> {
        let info = self
            .skills
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| crate::AiError::Other(format!("skill not found: {name}")))?;

        let content = tokio::fs::read_to_string(&info.path)
            .await
            .map_err(|e| crate::AiError::Other(format!("failed to read skill file: {e}")))?;

        match parse_skill_md(&content) {
            Some((_, body)) => Ok(body),
            None => Err(crate::AiError::Other(
                "failed to parse skill frontmatter".into(),
            )),
        }
    }

    /// Find skills whose name or description contains `query` (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<&SkillInfo> {
        let q = query.to_lowercase();
        self.skills
            .iter()
            .filter(|s| s.name.to_lowercase().contains(&q) || s.description.to_lowercase().contains(&q))
            .collect()
    }
}

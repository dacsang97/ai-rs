#[derive(Debug, Clone)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
}

/// Parse a SKILL.md file content, extracting YAML frontmatter and the body.
///
/// Returns `(frontmatter, body)` where body is the markdown content after the
/// closing `---` fence. Returns `None` if frontmatter is missing or invalid.
pub fn parse_skill_md(content: &str) -> Option<(SkillFrontmatter, String)> {
    let trimmed = content.trim_start();

    // Must start with "---"
    let rest = trimmed.strip_prefix("---")?;
    let rest = rest.strip_prefix('\n').or_else(|| rest.strip_prefix("\r\n"))?;

    // Find closing "---"
    let closing = rest.find("\n---")?;
    let frontmatter_str = &rest[..closing];
    let after_closing = &rest[closing + 4..]; // skip "\n---"

    // Body starts after the closing fence line
    let body = after_closing
        .strip_prefix('\n')
        .or_else(|| after_closing.strip_prefix("\r\n"))
        .unwrap_or(after_closing);

    let fm = parse_frontmatter(frontmatter_str)?;
    Some((fm, body.to_string()))
}

/// Parse key-value pairs from frontmatter text.
/// Handles `key: value`, `key: "quoted value"`, and multi-line values via
/// indented continuation lines.
fn parse_frontmatter(text: &str) -> Option<SkillFrontmatter> {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut current_key: Option<String> = None;
    let mut current_value = String::new();

    for line in text.lines() {
        if !line.starts_with(' ') && !line.starts_with('\t') {
            // Flush previous key-value
            if let Some(key) = current_key.take() {
                store_field(&key, &current_value, &mut name, &mut description);
                current_value.clear();
            }

            // Parse new key: value
            if let Some((key, val)) = line.split_once(':') {
                let key = key.trim().to_string();
                let val = strip_quotes(val.trim());
                current_key = Some(key);
                current_value = val;
            }
        } else {
            // Continuation line for multi-line value
            if current_key.is_some() {
                if !current_value.is_empty() {
                    current_value.push(' ');
                }
                current_value.push_str(line.trim());
            }
        }
    }

    // Flush last key-value
    if let Some(key) = current_key.take() {
        store_field(&key, &current_value, &mut name, &mut description);
    }

    Some(SkillFrontmatter {
        name: name?,
        description: description?,
    })
}

fn store_field(
    key: &str,
    value: &str,
    name: &mut Option<String>,
    description: &mut Option<String>,
) {
    match key {
        "name" => *name = Some(value.to_string()),
        "description" => *description = Some(value.to_string()),
        _ => {}
    }
}

fn strip_quotes(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_frontmatter() {
        let content = r#"---
name: my-skill
description: A test skill
---
# My Skill

Body content here.
"#;
        let (fm, body) = parse_skill_md(content).unwrap();
        assert_eq!(fm.name, "my-skill");
        assert_eq!(fm.description, "A test skill");
        assert!(body.contains("Body content here."));
    }

    #[test]
    fn parse_quoted_values() {
        let content = "---\nname: \"quoted-name\"\ndescription: 'quoted desc'\n---\nbody\n";
        let (fm, _) = parse_skill_md(content).unwrap();
        assert_eq!(fm.name, "quoted-name");
        assert_eq!(fm.description, "quoted desc");
    }

    #[test]
    fn parse_multiline_description() {
        let content = "---\nname: multi\ndescription: line one\n  line two\n  line three\n---\nbody\n";
        let (fm, _) = parse_skill_md(content).unwrap();
        assert_eq!(fm.name, "multi");
        assert_eq!(fm.description, "line one line two line three");
    }

    #[test]
    fn missing_frontmatter_returns_none() {
        assert!(parse_skill_md("# No frontmatter").is_none());
    }

    #[test]
    fn missing_required_field_returns_none() {
        let content = "---\nname: only-name\n---\nbody\n";
        assert!(parse_skill_md(content).is_none());
    }
}

use ai_rs::skill::parse_skill_md;

#[test]
fn parse_valid_frontmatter() {
    let content = r#"---
name: my-skill
description: A helpful skill
---
# My Skill

Instructions here.
"#;
    let (fm, body) = parse_skill_md(content).unwrap();
    assert_eq!(fm.name, "my-skill");
    assert_eq!(fm.description, "A helpful skill");
    assert!(body.contains("Instructions here."));
}

#[test]
fn parse_missing_name_returns_none() {
    let content = "---\ndescription: no name field\n---\nbody\n";
    assert!(parse_skill_md(content).is_none());
}

#[test]
fn parse_missing_description_returns_none() {
    let content = "---\nname: only-name\n---\nbody\n";
    assert!(parse_skill_md(content).is_none());
}

#[test]
fn parse_no_frontmatter_returns_none() {
    assert!(parse_skill_md("# Just markdown").is_none());
}

#[test]
fn parse_no_closing_fence_returns_none() {
    let content = "---\nname: broken\ndescription: no close\n";
    assert!(parse_skill_md(content).is_none());
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
    assert_eq!(fm.description, "line one line two line three");
}

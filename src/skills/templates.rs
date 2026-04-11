use crate::bigquery::dynamic::model::{GeneratedCommand, ParamLocation};
use crate::commands::meta::{CommandContract, FlagContract};

/// Maximum skill name length (agentskills.io constraint).
const MAX_SKILL_NAME_LEN: usize = 64;

/// Maximum lines in the thin router SKILL.md.
const MAX_SKILL_MD_LINES: usize = 60;

/// Flags handled elsewhere (Prerequisites, usage line, or internal) —
/// not listed in per-command flag tables.
const SKIP_FLAGS: &[&str] = &[
    "--project-id",
    "--dataset-id",
    "--location",
    "--format",
    "--table",
    "--token",
    "--credentials-file",
    "--sanitize",
    "--page-token",
    "--page-all",
    "--yes",
    "--dry-run", // shown separately in usage line
];

/// A generated skill ready to be written to disk.
#[derive(Debug, Clone)]
pub struct SkillOutput {
    /// Skill directory name, e.g. "dcx-datasets"
    pub dir_name: String,
    /// SKILL.md content (thin router)
    pub skill_md: String,
    /// agents/openai.yaml content
    pub openai_yaml: String,
    /// references/commands.md content (full command detail)
    pub references_md: String,
}

/// agentskills.io validation errors.
#[derive(Debug)]
pub struct SkillValidation {
    pub skill_name: String,
    pub errors: Vec<String>,
}

/// Generate a skill for a resource group (e.g. "datasets") from its commands,
/// using contracts as the source of truth for flags and constraints.
pub fn generate_resource_skill(
    group: &str,
    commands: &[&GeneratedCommand],
    contracts: &[CommandContract],
) -> SkillOutput {
    let dir_name = format!("dcx-{group}");
    let display_name = capitalize(group);

    let skill_md = build_thin_skill_md(group, commands, contracts);
    let openai_yaml = build_openai_yaml(group, &display_name, commands);
    let references_md = build_references_md(group, commands, contracts);

    SkillOutput {
        dir_name,
        skill_md,
        openai_yaml,
        references_md,
    }
}

/// Validate a skill against agentskills.io constraints.
pub fn validate_skill(skill: &SkillOutput) -> SkillValidation {
    let mut errors = Vec::new();

    // Name must be lowercase-hyphenated.
    if skill.dir_name != skill.dir_name.to_lowercase() {
        errors.push(format!("Name '{}' must be lowercase", skill.dir_name));
    }
    if skill.dir_name.contains('_') {
        errors.push(format!(
            "Name '{}' must use hyphens, not underscores",
            skill.dir_name
        ));
    }

    // Name length check.
    if skill.dir_name.len() > MAX_SKILL_NAME_LEN {
        errors.push(format!(
            "Name '{}' exceeds {} chars ({} chars)",
            skill.dir_name,
            MAX_SKILL_NAME_LEN,
            skill.dir_name.len()
        ));
    }

    // Must have trigger condition (When to use section).
    if !skill.skill_md.contains("## When to use this skill") {
        errors.push("Missing '## When to use this skill' section".to_string());
    }

    // Frontmatter must have name and description.
    if !skill.skill_md.contains("name: ") {
        errors.push("Missing 'name:' in frontmatter".to_string());
    }
    if !skill.skill_md.contains("description: ") {
        errors.push("Missing 'description:' in frontmatter".to_string());
    }

    // Size budget: router SKILL.md must stay within line cap.
    let line_count = skill.skill_md.lines().count();
    if line_count > MAX_SKILL_MD_LINES {
        errors.push(format!(
            "SKILL.md exceeds {} line budget ({} lines)",
            MAX_SKILL_MD_LINES, line_count
        ));
    }

    // Flag tables must not appear in SKILL.md (they belong in references).
    if skill.skill_md.contains("| Flag | Required | Description |") {
        errors.push("SKILL.md contains flag table — move to references/commands.md".to_string());
    }

    // Must point to references.
    if !skill.skill_md.contains("references/commands.md") {
        errors.push("SKILL.md must reference references/commands.md".to_string());
    }

    SkillValidation {
        skill_name: skill.dir_name.clone(),
        errors,
    }
}

// ---------------------------------------------------------------------------
// Thin router SKILL.md (routing knowledge only)
// ---------------------------------------------------------------------------

fn build_thin_skill_md(
    group: &str,
    commands: &[&GeneratedCommand],
    contracts: &[CommandContract],
) -> String {
    let mut out = String::new();

    // Frontmatter
    let action_list: Vec<&str> = commands.iter().map(|c| c.action.as_str()).collect();
    let actions_str = action_list.join(", ");
    out.push_str(&format!(
        "---\n\
         name: dcx-{group}\n\
         description: Use dcx to manage BigQuery {group} via the {actions_str} commands.\n\
         ---\n\n"
    ));

    // When to use
    out.push_str("## When to use this skill\n\n");
    out.push_str("Use when the user wants to:\n");
    for cmd in commands {
        out.push_str(&format!(
            "- {} a BigQuery {}\n",
            cmd.action,
            singular(group)
        ));
    }
    out.push_str(&format!("- show or inspect BigQuery {group}\n\n"));
    out.push_str(
        "Do not use for analytics workflows (doctor, evaluate, get-trace) \
         — use dcx-analytics instead.\n\n",
    );

    // Prerequisites
    out.push_str("## Prerequisites\n\n");
    out.push_str("Authentication: `dcx auth login` or set `DCX_PROJECT` / `DCX_TOKEN`.\n\n");

    let all_need_dataset = commands.iter().all(|c| cmd_needs_dataset(c));
    let some_need_dataset = commands.iter().any(|c| cmd_needs_dataset(c));
    if all_need_dataset {
        out.push_str("Requires: `--project-id` and `--dataset-id`\n\n");
    } else if some_need_dataset {
        out.push_str("Requires: `--project-id` (all), `--dataset-id` (some)\n\n");
    } else {
        out.push_str("Requires: `--project-id`\n\n");
    }

    // Command summary table (no flags — just names and descriptions)
    out.push_str("## Commands\n\n");
    out.push_str("| Command | Description |\n");
    out.push_str("|---------|-------------|\n");
    for cmd in commands {
        let cmd_path = format!("dcx {} {}", group, cmd.action);
        let description = contracts
            .iter()
            .find(|c| c.command == cmd_path)
            .map(|c| truncate_description(&c.synopsis))
            .unwrap_or_else(|| truncate_description(&cmd.about));
        out.push_str(&format!("| `{cmd_path}` | {description} |\n"));
    }
    out.push_str(
        "\nSee [references/commands.md](references/commands.md) for flags, constraints, and examples.\n\n",
    );

    // Decision rules
    out.push_str("## Decision rules\n\n");
    out.push_str("- Use `--dry-run` to preview the API request\n");
    out.push_str("- Use `--format table` for visual scanning, `--format json` for piping\n");
    out.push_str(
        "- Use `--format json-minified` for agent pipelines (same schema, ~27% fewer tokens)\n",
    );
    if commands.iter().any(|c| c.action == "list") {
        out.push_str(&format!(
            "- Use `{group} list` to discover available {group}\n"
        ));
    }
    if commands.iter().any(|c| c.action == "get") {
        out.push_str(&format!(
            "- Use `{group} get` to inspect a specific {singular}\n",
            singular = singular(group)
        ));
    }

    out
}

// ---------------------------------------------------------------------------
// Generated references/commands.md (full command detail)
// ---------------------------------------------------------------------------

fn build_references_md(
    group: &str,
    commands: &[&GeneratedCommand],
    contracts: &[CommandContract],
) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "# dcx-{group} command reference\n\n\
         > Generated from the command contract. Do not edit by hand.\n\n"
    ));

    for cmd in commands {
        let needs_dataset = cmd_needs_dataset(cmd);
        let cmd_path = format!("dcx {} {}", group, cmd.action);
        let contract = contracts.iter().find(|c| c.command == cmd_path);

        out.push_str(&format!("## {} {}\n\n", group, cmd.action));
        let description = contract
            .map(|c| c.synopsis.as_str())
            .unwrap_or(cmd.about.as_str());
        out.push_str(&format!("{}\n\n", description));

        // Collect flags from contract.
        let contract_flags: Vec<&FlagContract> = if let Some(c) = contract {
            c.flags
                .iter()
                .chain(c.global_flags.iter())
                .filter(|f| !SKIP_FLAGS.contains(&f.name.as_str()))
                .collect()
        } else {
            Vec::new()
        };

        // Usage block.
        let mut usage = format!("```bash\ndcx {group} {}", cmd.action);
        usage.push_str(" \\\n  --project-id <PROJECT_ID>");
        if needs_dataset {
            usage.push_str(" \\\n  --dataset-id <DATASET_ID>");
        }
        for flag in &contract_flags {
            if flag.required {
                let placeholder = flag
                    .name
                    .trim_start_matches("--")
                    .to_uppercase()
                    .replace('-', "_");
                usage.push_str(&format!(" \\\n  {} <{}>", flag.name, placeholder));
            }
        }
        for flag in &contract_flags {
            if !flag.required {
                if flag.flag_type == "boolean" {
                    usage.push_str(&format!(" \\\n  [{}]", flag.name));
                } else {
                    let placeholder = flag
                        .name
                        .trim_start_matches("--")
                        .to_uppercase()
                        .replace('-', "_");
                    usage.push_str(&format!(" \\\n  [{} <{}>]", flag.name, placeholder));
                }
            }
        }
        if contract.map(|c| c.supports_dry_run).unwrap_or(true) {
            usage.push_str(" \\\n  [--dry-run]");
        }
        usage.push_str(" \\\n  [--format json|json-minified|table|text]\n```\n\n");
        out.push_str(&usage);

        // Flags table.
        out.push_str("| Flag | Required | Description |\n");
        out.push_str("|------|----------|-------------|\n");
        out.push_str("| `--project-id` | Yes | GCP project ID (global flag) |\n");
        if needs_dataset {
            out.push_str("| `--dataset-id` | Yes | BigQuery dataset (global flag) |\n");
        }
        for flag in &contract_flags {
            let req = if flag.required { "Yes" } else { "No" };
            let desc = truncate_description(&flag.description);
            out.push_str(&format!("| `{}` | {} | {} |\n", flag.name, req, desc));
        }
        out.push('\n');

        // Constraints from contract.
        if let Some(c) = contract {
            if !c.constraints.is_empty() {
                out.push_str("**Constraints:**\n");
                for constraint in &c.constraints {
                    let flags = constraint.flags.join(", ");
                    out.push_str(&format!(
                        "- {} ({}): {}\n",
                        constraint.constraint_type, flags, constraint.description
                    ));
                }
                out.push('\n');
            }
        }
    }

    // Examples
    out.push_str("## Examples\n\n```bash\n");
    for cmd in commands {
        let cmd_needs_dataset = cmd
            .method
            .parameters
            .iter()
            .any(|p| p.name == "datasetId" && p.location == ParamLocation::Path);

        let cmd_path = format!("dcx {} {}", group, cmd.action);
        let contract = contracts.iter().find(|c| c.command == cmd_path);

        out.push_str(&format!("# {} {}\n", capitalize(&cmd.action), group));
        out.push_str(&format!("dcx {group} {}", cmd.action));
        out.push_str(" \\\n  --project-id my-proj");

        if cmd_needs_dataset {
            out.push_str(" \\\n  --dataset-id my_dataset");
        }

        if let Some(c) = contract {
            for flag in &c.flags {
                if flag.required && !SKIP_FLAGS.contains(&flag.name.as_str()) {
                    let val = format!(
                        "my_{}",
                        flag.name.trim_start_matches("--").replace('-', "_")
                    );
                    out.push_str(&format!(" \\\n  {} {}", flag.name, val));
                }
            }
        }

        out.push_str(" \\\n  --format table\n\n");
    }
    out.push_str("```\n");

    out
}

fn build_openai_yaml(group: &str, display_name: &str, commands: &[&GeneratedCommand]) -> String {
    let action_list: Vec<&str> = commands.iter().map(|c| c.action.as_str()).collect();
    let actions_str = action_list.join("/");

    format!(
        "interface:\n  \
         display_name: \"dcx {display_name}\"\n  \
         short_description: \"{actions_str} BigQuery {group} via dcx\"\n  \
         default_prompt: \"Use $dcx-{group} to {actions_str} BigQuery {group} using the dcx CLI.\"\n\n\
         policy:\n  \
         allow_implicit_invocation: true\n"
    )
}

/// Check if a command requires datasetId as a path parameter.
fn cmd_needs_dataset(cmd: &GeneratedCommand) -> bool {
    cmd.method
        .parameters
        .iter()
        .any(|p| p.name == "datasetId" && p.location == ParamLocation::Path)
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

fn singular(group: &str) -> &str {
    match group {
        "datasets" => "dataset",
        "tables" => "table",
        "jobs" => "job",
        "routines" => "routine",
        "models" => "model",
        _ => group,
    }
}

/// Truncate Discovery descriptions to keep table cells readable.
fn truncate_description(s: &str) -> String {
    let first_sentence = s.split(". ").next().unwrap_or(s);
    if first_sentence.len() > 120 {
        format!("{}...", &first_sentence[..117])
    } else {
        first_sentence.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitalize_works() {
        assert_eq!(capitalize("datasets"), "Datasets");
        assert_eq!(capitalize("tables"), "Tables");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn singular_works() {
        assert_eq!(singular("datasets"), "dataset");
        assert_eq!(singular("tables"), "table");
        assert_eq!(singular("unknown"), "unknown");
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate_description("A short desc"), "A short desc");
    }

    #[test]
    fn truncate_multi_sentence() {
        let desc = "First sentence. Second sentence. Third.";
        assert_eq!(truncate_description(desc), "First sentence");
    }

    #[test]
    fn truncate_long_single_sentence() {
        let long = "A".repeat(200);
        let result = truncate_description(&long);
        assert!(result.len() <= 120);
        assert!(result.ends_with("..."));
    }
}

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::process::Command;

#[derive(Parser)]
#[command(
    name = "git-commit-message",
    about = "Generate conventional commit messages using Claude AI",
    version
)]
struct Cli {
    /// Claude model to use
    #[arg(short, long, default_value = "haiku")]
    model: String,

    /// Number of commit message suggestions to generate
    #[arg(short = 'n', long, default_value_t = 10)]
    count: u8,

    /// Preview suggestions without committing
    #[arg(short, long)]
    dry_run: bool,
}

fn get_staged_diff() -> Result<String> {
    let check = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .context("Failed to run git")?;
    if !check.status.success() {
        bail!("Not inside a git repository");
    }

    let output = Command::new("git")
        .args(["diff", "--cached"])
        .output()
        .context("Failed to run git diff")?;

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    if diff.trim().is_empty() {
        bail!("No staged changes found. Use 'git add' to stage files first.");
    }

    Ok(diff.chars().take(8000).collect())
}

fn get_recent_commits() -> Result<String> {
    let output = Command::new("git")
        .args(["log", "-n", "10", "--pretty=format:%h %s"])
        .output()
        .context("Failed to run git log")?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn generate_commit_messages(diff: &str, commits: &str, model: &str, count: u8) -> Result<Vec<String>> {
    let prompt = format!(
        r#"Please suggest {count} commit messages, given the following diff:

```diff
{diff}
```

**Criteria:**
1. **Format:** `<type>(<scope>): <description>` (conventional commits)
2. **Relevance:** Avoid mentioning a module unless directly relevant
3. **Clarity and Conciseness:** Each message clearly conveys the change

**Recent Commits on Repo for Reference:**
```
{commits}
```

**Output Template**
Output ONLY raw commit messages, one per line, no numbering, no decorations.

fix(app): add password regex pattern
test(unit): add new test cases"#
    );

    let output = Command::new("claude")
        .args([
            "--dangerously-skip-permissions",
            "--model",
            model,
            "--no-session-persistence",
            "-p",
            &prompt,
        ])
        .output()
        .context("Failed to run claude CLI. Is it installed and in PATH?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("claude CLI failed: {stderr}");
    }

    let text = String::from_utf8_lossy(&output.stdout).to_string();
    parse_messages(&text, count)
}

fn parse_messages(text: &str, count: u8) -> Result<Vec<String>> {
    let messages: Vec<String> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| {
            // Strip leading "N. " or "N) " numbering
            if let Some(pos) = l.find(". ").or_else(|| l.find(") ")) {
                let prefix = &l[..pos];
                if prefix.chars().all(|c| c.is_ascii_digit()) {
                    return l[pos + 2..].to_string();
                }
            }
            l.to_string()
        })
        .take(count as usize)
        .collect();

    if messages.is_empty() {
        bail!("Failed to parse commit messages from response");
    }

    Ok(messages)
}

fn select_message(messages: &[String]) -> Result<String> {
    let index = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Select a commit message (↑↓ to navigate, Enter to confirm, Esc to cancel)")
        .items(messages)
        .default(0)
        .interact()?;

    Ok(messages[index].clone())
}

fn commit(message: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["commit", "-m", message])
        .status()
        .context("Failed to run git commit")?;

    if !status.success() {
        bail!("git commit failed");
    }

    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    eprintln!("Analyzing staged changes...");
    let diff = get_staged_diff()?;
    let commits = get_recent_commits()?;

    eprintln!("Generating {} commit messages with Claude ({})...", cli.count, cli.model);
    let messages = generate_commit_messages(&diff, &commits, &cli.model, cli.count)?;

    if cli.dry_run {
        println!("Suggested commit messages:");
        for (i, msg) in messages.iter().enumerate() {
            println!("  {}. {}", i + 1, msg);
        }
        return Ok(());
    }

    let selected = select_message(&messages)?;
    commit(&selected)?;

    Ok(())
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}

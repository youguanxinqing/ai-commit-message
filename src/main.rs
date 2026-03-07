use anyhow::{bail, Context, Result};
use clap::Parser;
use console::style;
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

    /// Print time taken for AI generation
    #[arg(short, long)]
    timing: bool,
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

**Recent Commits on Repo for Reference:**
```
{commits}
```

**Criteria:**

1. **Format:** Each commit message must follow the conventional commits format,
which is \`<type>(<scope>): <description>\`.
2. **Relevance:** Avoid mentioning a module name unless it's directly relevant
to the change.
3. **Enumeration:** List the commit messages from 1 to {count}.
4. **Clarity and Conciseness:** Each message should clearly and concisely convey
the change made.

**Output Template**

Follow this output template and ONLY output raw commit messages without spacing,
numbers or other decorations.

fix(app): add password regex pattern
test(unit): add new test cases
style: remove unused imports
refactor(pages): extract common code to \`utils/wait.ts\`

**Instructions:**

- Take a moment to understand the changes made in the diff.

- Think about the impact of these changes on the project (e.g., bug fixes, new
features, performance improvements, code refactoring, documentation updates).
It's critical to my career you abstract the changes to a higher level and not
just describe the code changes.

- Generate commit messages that accurately describe these changes, ensuring they
are helpful to someone reading the project's history.

- Remember, a well-crafted commit message can significantly aid in the maintenance
and understanding of the project over time.

- If multiple changes are present, make sure you capture them all in each commit
message.

Keep in mind you will suggest {count} commit messages. Only 1 will be used. It's
better to push yourself (esp to synthesize to a higher level) and maybe wrong
about some of the {count} commits because only one needs to be good. I'm looking
for your best commit, not the best average commit. It's better to cover more
scenarios than include a lot of overlap.

Write your {count} commit messages below in the format shown in Output Template section above."#
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
    let start = std::time::Instant::now();
    let messages = generate_commit_messages(&diff, &commits, &cli.model, cli.count)?;
    if cli.timing {
        eprintln!("AI generation took {:.2}s", start.elapsed().as_secs_f64());
    }

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
        eprintln!("{} {e:#}", style("Error:").red().bold());
        std::process::exit(1);
    }
}

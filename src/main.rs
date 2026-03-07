use anyhow::{bail, Context, Result};
use clap::Parser;
use console::style;
use std::io::{BufRead, BufReader};
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

    /// Use direct HTTP API instead of claude CLI.
    /// Requires ANTHROPIC_API_KEY; ANTHROPIC_BASE_URL defaults to https://api.anthropic.com
    #[arg(long)]
    http: bool,
}

fn resolve_model(name: &str) -> &str {
    match name {
        "haiku" => "claude-haiku-4-5-20251001",
        "sonnet" => "claude-sonnet-4-6",
        "opus" => "claude-opus-4-6",
        other => other,
    }
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

fn build_prompt(diff: &str, commits: &str, count: u8) -> String {
    format!(
        r#"Please suggest {count} commit messages, given the following diff:

```diff
{diff}
```

**Criteria:**

1. **Format:** Each commit message must follow the conventional commits format,
which is `<type>(<scope>): <description>`.
2. **Relevance:** Avoid mentioning a module name unless it's directly relevant
to the change.
3. **Enumeration:** List the commit messages from 1 to {count}.
4. **Clarity and Conciseness:** Each message should clearly and concisely convey
the change made.

**Commit Message Examples:**

- fix(app): add password regex pattern
- test(unit): add new test cases
- style: remove unused imports
- refactor(pages): extract common code to `utils/wait.ts`

**Recent Commits on Repo for Reference:**

```
{commits}
```

**Output Template**

Follow this output template and ONLY output raw commit messages without spacing,
numbers or other decorations.

fix(app): add password regex pattern
test(unit): add new test cases
style: remove unused imports
refactor(pages): extract common code to `utils/wait.ts`

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
    )
}

fn strip_numbering(messages: Vec<String>, count: u8) -> Result<Vec<String>> {
    let cleaned: Vec<String> = messages
        .into_iter()
        .map(|l| {
            if let Some(pos) = l.find(". ").or_else(|| l.find(") ")) {
                let prefix = &l[..pos];
                if prefix.chars().all(|c| c.is_ascii_digit()) {
                    return l[pos + 2..].to_string();
                }
            }
            l
        })
        .take(count as usize)
        .collect();

    if cleaned.is_empty() {
        bail!("No commit messages in response");
    }

    Ok(cleaned)
}

fn generate_via_cli(diff: &str, commits: &str, model: &str, count: u8, timing: bool) -> Result<Vec<String>> {
    let prompt = build_prompt(diff, commits, count);
    let start = std::time::Instant::now();

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

    if timing {
        eprintln!("AI generation took {:.2}s", start.elapsed().as_secs_f64());
    }

    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    strip_numbering(lines, count)
}

fn generate_via_http(
    diff: &str,
    commits: &str,
    model: &str,
    count: u8,
    timing: bool,
) -> Result<Vec<String>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        anyhow::anyhow!(
            "ANTHROPIC_API_KEY is not set\n  Set it with: export ANTHROPIC_API_KEY=sk-ant-..."
        )
    })?;
    let base_url = std::env::var("ANTHROPIC_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".to_string());

    let model_id = resolve_model(model);
    let prompt = build_prompt(diff, commits, count);
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model_id,
        "max_tokens": 1024,
        "stream": true,
        "system": "You are an expert at writing git commit messages following the Conventional Commits specification.",
        "messages": [{"role": "user", "content": prompt}]
    });

    let start = std::time::Instant::now();

    let response = ureq::post(&url)
        .set("x-api-key", &api_key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_json(&body)
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let body = resp.into_string().unwrap_or_default();
                anyhow::anyhow!("Anthropic API error {code}: {body}")
            }
            other => anyhow::anyhow!("HTTP request failed: {other}"),
        })?;

    let reader = BufReader::new(response.into_reader());
    let mut full_text = String::new();
    let mut last_newline_pos = 0usize;
    let mut messages: Vec<String> = Vec::new();
    let mut first_elapsed: Option<std::time::Duration> = None;

    for line in reader.lines() {
        let line = line.context("Failed to read SSE stream")?;
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if event["type"] == "content_block_delta" && event["delta"]["type"] == "text_delta" {
            if let Some(text) = event["delta"]["text"].as_str() {
                full_text.push_str(text);
                while let Some(rel) = full_text[last_newline_pos..].find('\n') {
                    let abs = last_newline_pos + rel;
                    let msg = full_text[last_newline_pos..abs].trim().to_string();
                    last_newline_pos = abs + 1;
                    if msg.is_empty() {
                        continue;
                    }
                    if first_elapsed.is_none() {
                        first_elapsed = Some(start.elapsed());
                    }
                    messages.push(msg);
                }
            }
        }
    }

    // Capture any trailing text not ending with \n
    let tail = full_text[last_newline_pos..].trim().to_string();
    if !tail.is_empty() {
        if first_elapsed.is_none() {
            first_elapsed = Some(start.elapsed());
        }
        messages.push(tail);
    }

    if timing {
        match first_elapsed {
            Some(first) => eprintln!(
                "AI generation: first message in {:.2}s, all {} in {:.2}s",
                first.as_secs_f64(),
                messages.len(),
                start.elapsed().as_secs_f64()
            ),
            None => eprintln!(
                "AI generation: {:.2}s total (no messages received)",
                start.elapsed().as_secs_f64()
            ),
        }
    }

    strip_numbering(messages, count)
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

    let messages = if cli.http {
        eprintln!(
            "Generating {} commit messages via HTTP ({})...",
            cli.count,
            resolve_model(&cli.model)
        );
        generate_via_http(&diff, &commits, &cli.model, cli.count, cli.timing)?
    } else {
        eprintln!(
            "Generating {} commit messages via claude CLI ({})...",
            cli.count, cli.model
        );
        generate_via_cli(&diff, &commits, &cli.model, cli.count, cli.timing)?
    };

    eprintln!();

    if cli.dry_run {
        for (i, msg) in messages.iter().enumerate() {
            eprintln!("  {}. {}", i + 1, style(msg).cyan());
        }
        eprintln!();
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

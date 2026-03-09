use anyhow::{Context, Result, bail};
use clap::Parser;
use dialoguer::console::{Key, Term, measure_text_width, style};
use std::io::{BufRead, BufReader};
use std::process::Command;

#[derive(Parser)]
#[command(
    name = "ai-commit-message",
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

    /// Print the prompt sent to the AI
    #[arg(short, long)]
    verbose: bool,
}

enum Selection {
    Picked(String),
    Feedback(String),
    Cancelled,
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

fn print_verbose(prompt: &str) {
    let width = Term::stderr().size().1.max(60).min(100) as usize;
    let divider = style("─".repeat(width)).dim();

    eprintln!();
    eprintln!("{divider}");
    eprintln!(
        "  {}  {}",
        style("PROMPT").bold().yellow(),
        style(format!("· {} chars", prompt.len())).dim()
    );
    eprintln!("{divider}");
    eprintln!();

    let mut in_code_block = false;
    for line in prompt.lines() {
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            eprintln!("  {}", style(line).dim());
            continue;
        }
        if in_code_block {
            eprintln!("  {}", style(line).green());
        } else if line.starts_with("**") {
            eprintln!("  {}", style(line).bold().cyan());
        } else if line.starts_with("- ") {
            eprintln!("  {}", style(line).dim());
        } else if line.trim().is_empty() {
            eprintln!();
        } else {
            eprintln!("  {}", style(line).dim());
        }
    }

    eprintln!();
    eprintln!("{divider}");
    eprintln!();
}

fn read_feedback_line(term: &Term) -> Result<String> {
    const PREFIX: &str = "  > ";

    // We must calculate terminal rows by display width (CJK can be width 2), not bytes/chars.
    fn rendered_rows(term: &Term, chars: &[char]) -> usize {
        let width = term.size().1.max(1) as usize;
        let mut columns = measure_text_width(PREFIX);
        for ch in chars {
            columns += measure_text_width(&ch.to_string()).max(1);
        }
        let columns = columns.max(1);
        ((columns - 1) / width) + 1
    }

    fn redraw_feedback_line(term: &Term, chars: &[char], old_rows: &mut usize) -> Result<()> {
        let rows_before_current = old_rows.saturating_sub(1);

        if rows_before_current > 0 {
            // clear_last_lines only clears lines before current line and leaves cursor at the
            // first cleared line, so we must also clear current line explicitly.
            term.clear_last_lines(rows_before_current)?;
            term.move_cursor_down(rows_before_current)?;
        }
        term.clear_line()?;
        if rows_before_current > 0 {
            term.move_cursor_up(rows_before_current)?;
        }

        term.write_str(PREFIX)?;
        if !chars.is_empty() {
            let current: String = chars.iter().collect();
            term.write_str(&current)?;
        }
        term.flush()?;
        *old_rows = rendered_rows(term, chars);
        Ok(())
    }

    let mut chars: Vec<char> = Vec::new();
    let mut saw_delete_prefix = false;
    let mut swallow_delete_suffix = false;
    let mut rows = 1usize;

    term.write_str(PREFIX)?;
    term.flush()?;

    loop {
        let key = term.read_key()?;
        match key {
            Key::Enter => {
                term.write_line("")?;
                break;
            }
            Key::Backspace | Key::Del => {
                saw_delete_prefix = false;
                swallow_delete_suffix = false;
                if chars.pop().is_some() {
                    redraw_feedback_line(term, &chars, &mut rows)?;
                }
            }
            // Some terminals split Delete (\x1b[3~) across reads:
            // UnknownEscSeq(['[']) then '3' then '~'. Treat all these forms as one delete key.
            // This prevents "cannot fully delete text" when escape sequence arrives in fragments.
            Key::UnknownEscSeq(seq) if seq.as_slice() == ['[', '3', '~'] => {
                saw_delete_prefix = false;
                swallow_delete_suffix = false;
                if chars.pop().is_some() {
                    redraw_feedback_line(term, &chars, &mut rows)?;
                }
            }
            Key::UnknownEscSeq(seq) if seq.as_slice() == ['[', '3'] => {
                saw_delete_prefix = false;
                swallow_delete_suffix = true;
                if chars.pop().is_some() {
                    redraw_feedback_line(term, &chars, &mut rows)?;
                }
            }
            Key::UnknownEscSeq(seq) if seq.as_slice() == ['['] => {
                saw_delete_prefix = true;
                swallow_delete_suffix = false;
            }
            Key::Char('3') if saw_delete_prefix => {
                saw_delete_prefix = false;
                swallow_delete_suffix = true;
                if chars.pop().is_some() {
                    redraw_feedback_line(term, &chars, &mut rows)?;
                }
            }
            Key::Char('~') if swallow_delete_suffix => {
                saw_delete_prefix = false;
                swallow_delete_suffix = false;
            }
            Key::Char(ch) if !ch.is_ascii_control() => {
                saw_delete_prefix = false;
                swallow_delete_suffix = false;
                chars.push(ch);
                redraw_feedback_line(term, &chars, &mut rows)?;
            }
            _ => {
                saw_delete_prefix = false;
                swallow_delete_suffix = false;
            }
        }
    }

    Ok(chars.iter().collect::<String>().trim().to_string())
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

fn generate_via_cli(prompt: &str, model: &str, count: u8, timing: bool) -> Result<Vec<String>> {
    let start = std::time::Instant::now();

    let output = Command::new("claude")
        .args([
            "--dangerously-skip-permissions",
            "--model",
            model,
            "--no-session-persistence",
            "-p",
            prompt,
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

fn generate_via_http(prompt: &str, model: &str, count: u8, timing: bool) -> Result<Vec<String>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        anyhow::anyhow!(
            "ANTHROPIC_API_KEY is not set\n  Set it with: export ANTHROPIC_API_KEY=sk-ant-..."
        )
    })?;
    let base_url = std::env::var("ANTHROPIC_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".to_string());

    let model_id = resolve_model(model);
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

fn select_message(messages: &[String]) -> Result<Selection> {
    const FEEDBACK_LABEL: &str = "↩  None of these — provide feedback...";

    let mut items: Vec<&str> = messages.iter().map(String::as_str).collect();
    items.push("");
    items.push(FEEDBACK_LABEL);

    let choice = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Select a commit message (↑↓ to navigate, Enter to confirm, Esc to cancel)")
        .items(&items)
        .default(0)
        .interact_opt()?;

    match choice {
        None => Ok(Selection::Cancelled),
        Some(i) if i < messages.len() => Ok(Selection::Picked(messages[i].clone())),
        Some(_) => {
            let term = Term::stderr();
            // Ensure cursor is visible before taking free-form feedback input.
            let _ = term.show_cursor();
            eprintln!("  {}", style("How should the messages be improved?").bold());
            // Use key-by-key input to keep CJK backspace/delete behavior deterministic.
            let hint = read_feedback_line(&term)?;
            if hint.is_empty() {
                return Ok(Selection::Cancelled);
            }
            Ok(Selection::Feedback(hint))
        }
    }
}

fn build_retry_prompt(prev_prompt: &str, previous: &[String], hint: &str) -> String {
    let suggestions = previous
        .iter()
        .enumerate()
        .map(|(i, m)| format!("{}. {}", i + 1, m))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "{prev_prompt}

---

You previously suggested these messages:

{suggestions}

The user wants to improve them with this feedback: {hint}

Please suggest a fresh set of commit messages that address this feedback. \
Follow the same output format as before."
    )
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

    let mut prompt = build_prompt(&diff, &commits, cli.count);

    loop {
        if cli.verbose {
            print_verbose(&prompt);
        }

        let messages = if cli.http {
            eprintln!(
                "Generating {} commit messages via HTTP ({})...",
                cli.count,
                resolve_model(&cli.model)
            );
            generate_via_http(&prompt, &cli.model, cli.count, cli.timing)?
        } else {
            eprintln!(
                "Generating {} commit messages via claude CLI ({})...",
                cli.count, cli.model
            );
            generate_via_cli(&prompt, &cli.model, cli.count, cli.timing)?
        };

        eprintln!();

        if cli.dry_run {
            for (i, msg) in messages.iter().enumerate() {
                eprintln!("  {}. {}", i + 1, style(msg).cyan());
            }
            eprintln!();
            return Ok(());
        }

        match select_message(&messages)? {
            Selection::Picked(msg) => {
                commit(&msg)?;
                return Ok(());
            }
            Selection::Cancelled => {
                eprintln!("Cancelled.");
                return Ok(());
            }
            Selection::Feedback(hint) => {
                eprintln!();
                eprintln!("Regenerating with your feedback...");
                prompt = build_retry_prompt(&prompt, &messages, &hint);
                // loop continues
            }
        }
    }
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("{} {e:#}", style("Error:").red().bold());
        std::process::exit(1);
    }
}

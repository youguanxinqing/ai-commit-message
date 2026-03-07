# ai-commit-message

A CLI tool that generates [Conventional Commits](https://www.conventionalcommits.org/) messages from your staged changes using Claude AI.

Stage your files, run the command, pick a message, done.

## Installation

```sh
cargo install --path .
```

## Usage

```sh
ai-commit-message [OPTIONS]
```

Stage some files first, then run:

```sh
git add src/auth.rs
ai-commit-message
```

An interactive list of 10 suggested commit messages appears. Use `↑↓` to navigate and `Enter` to commit.

## Options

| Flag | Description |
|---|---|
| `-m, --model <MODEL>` | Claude model: `haiku` (default), `sonnet`, `opus`, or a full model ID |
| `-n, --count <N>` | Number of suggestions to generate (default: `10`) |
| `--http` | Use the Anthropic HTTP API directly instead of the `claude` CLI |
| `-d, --dry-run` | Print suggestions without committing |
| `-v, --verbose` | Print the full prompt sent to the AI |
| `-t, --timing` | Print how long AI generation took |

## Backends

### Default — Claude CLI

Requires the [`claude`](https://claude.ai/download) CLI to be installed and authenticated.

```sh
ai-commit-message
ai-commit-message --model sonnet
```

### HTTP — Direct API

Requires an Anthropic API key. Set the environment variables before running:

```sh
export ANTHROPIC_API_KEY=sk-ant-...
export ANTHROPIC_BASE_URL=https://api.anthropic.com  # optional, this is the default

ai-commit-message --http
ai-commit-message --http --model sonnet
```

The HTTP backend uses streaming, so responses arrive faster.

## Examples

```sh
# Generate 5 suggestions using the default claude CLI backend
ai-commit-message -n 5

# Use the HTTP backend with Sonnet, print timing info
ai-commit-message --http --model sonnet --timing

# Preview suggestions without committing
ai-commit-message --dry-run

# Print the full prompt to inspect what gets sent to the AI
ai-commit-message --verbose --dry-run
```

## How it works

1. Reads `git diff --cached` to get staged changes
2. Reads `git log -n 10` for recent commit history (used as style reference)
3. Sends both to Claude with a structured prompt asking for 10 Conventional Commits suggestions
4. Presents an interactive selector in the terminal
5. Runs `git commit -m "<selected message>"` on confirmation

# mdcode

Markdown code extractor CLI (Rust 2024) that pulls fenced and inline code blocks from files or stdin. Designed to be pipe-friendly and usable in tooling (list/JSON modes).

## Features

- Fenced block extraction with optional fence preservation (`--fenced`)
- Inline code extraction behind `--inline`
- Language filtering (`--lang rust`) or language listing (`--lang` with no value)
- Index/range selection via `-n/--number`
- Line numbers across output formats with `--line-numbers`
- Output modes: raw (default), `--list`, `--json`
- Separator control via `--sep`, fence preservation via `--fenced`
- Input from files, stdin, or both (stdin processed first)

## Installation

The primary installation method is via Cargo from the Git repository:

```bash
cargo install --git https://github.com/Sector-F-Labs/mdcode
```

## Usage

```bash
# Help
mdcode --help

# Basic extraction (stdin)
cat sample.md | mdcode

# Filter by language and preserve fences
mdcode --lang rust --fenced README.md

# List languages present (no extraction)
mdcode --lang README.md

# Inline code and line numbers, stdin + file
cat sample.md | mdcode --inline --line-numbers README.md

# JSON output
mdcode --json docs/*.md > blocks.json
```

During development you can also run directly via Cargo: `cargo run -- --help`

## Development

```bash
cargo fmt
cargo check
```

License: BSD 3-Clause (see `LICENSE`).

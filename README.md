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

## Usage

```bash
# Help
cargo run -- --help

# Basic extraction (stdin)
cat sample.md | cargo run --

# Filter by language and preserve fences
cargo run -- --lang rust --fenced README.md

# List languages present (no extraction)
cargo run -- --lang README.md

# Inline code and line numbers, stdin + file
cat sample.md | cargo run -- --inline --line-numbers README.md

# JSON output
cargo run -- --json docs/*.md > blocks.json
```

## Development

```bash
cargo fmt
cargo check
```

License: BSD 3-Clause (see `LICENSE`).

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;

use clap::{ArgAction, Parser, ValueHint};
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(
    name = "mdcode",
    about = "Extract fenced and inline code blocks from Markdown",
    version
)]
struct Args {
    /// Target code block by index or range (e.g. 0, 1-3)
    #[arg(short = 'n', long = "number", value_name = "INDEX|RANGE")]
    number: Option<String>,

    /// Filter by language; omit value to list languages found
    #[arg(long = "lang", num_args = 0..=1, value_name = "LANG")]
    lang: Option<Option<String>>,

    /// Separator between blocks when printing multiple
    #[arg(long = "sep", default_value = "\n", value_name = "SEPARATOR")]
    separator: String,

    /// Preserve fences around output blocks
    #[arg(long = "fenced", action = ArgAction::SetTrue)]
    fenced: bool,

    /// Emit JSON instead of raw code
    #[arg(long = "json", action = ArgAction::SetTrue)]
    json: bool,

    /// List blocks with metadata
    #[arg(long = "list", action = ArgAction::SetTrue)]
    list: bool,

    /// Include inline code spans (backticks)
    #[arg(long = "inline", action = ArgAction::SetTrue)]
    inline: bool,

    /// Include source line numbers in output
    #[arg(long = "line-numbers", action = ArgAction::SetTrue)]
    line_numbers: bool,

    /// Input files; if omitted, read from stdin. When both are provided, stdin is processed first.
    #[arg(value_name = "FILE", value_hint = ValueHint::FilePath)]
    files: Vec<PathBuf>,
}

#[derive(Debug)]
enum LangSelector {
    All,
    List,
    Filter(String),
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum BlockKind {
    Fenced,
    Inline,
}

#[derive(Debug)]
struct InputSource {
    name: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct CodeBlock {
    index: usize,
    source: String,
    kind: BlockKind,
    lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<usize>,
    code: String,
}

#[derive(Debug)]
enum IndexFilter {
    Single(usize),
    Range { start: usize, end: usize },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let lang_selector = parse_lang_selector(&args.lang);

    let inputs = collect_inputs(&args)?;
    if inputs.is_empty() {
        eprintln!("No input provided. Pass files or pipe markdown into stdin.");
        std::process::exit(1);
    }

    let mut blocks = collect_blocks(inputs, args.inline);
    if let LangSelector::Filter(lang) = &lang_selector {
        blocks.retain(|b| matches_lang(b, lang));
    }

    if let Some(filter) = parse_index_filter(args.number.as_deref())? {
        blocks = apply_index_filter(blocks, filter);
    }

    if blocks.is_empty() {
        eprintln!("No matching code blocks found.");
        std::process::exit(1);
    }

    if let LangSelector::List = lang_selector {
        list_languages(&blocks);
        return Ok(());
    }

    if args.json {
        emit_json(&blocks, args.line_numbers)?;
        return Ok(());
    }

    if args.list {
        print_list(&blocks, args.line_numbers);
        return Ok(());
    }

    print_raw(&blocks, args.fenced, args.line_numbers, &args.separator);
    Ok(())
}

fn parse_lang_selector(arg: &Option<Option<String>>) -> LangSelector {
    match arg {
        None => LangSelector::All,
        Some(None) => LangSelector::List,
        Some(Some(lang)) => LangSelector::Filter(lang.to_lowercase()),
    }
}

fn collect_inputs(args: &Args) -> Result<Vec<InputSource>, Box<dyn std::error::Error>> {
    let mut sources = Vec::new();
    let mut read_stdin = !io::stdin().is_terminal();
    if args.files.is_empty() {
        read_stdin = true;
    }

    if read_stdin {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        if !buffer.is_empty() || args.files.is_empty() {
            sources.push(InputSource {
                name: "stdin".to_string(),
                content: buffer,
            });
        }
    }

    for path in &args.files {
        let content = fs::read_to_string(path)?;
        sources.push(InputSource {
            name: path.display().to_string(),
            content,
        });
    }

    Ok(sources)
}

fn collect_blocks(inputs: Vec<InputSource>, include_inline: bool) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    for input in inputs {
        let mut parsed = parse_blocks(&input, include_inline);
        blocks.append(&mut parsed);
    }

    for (index, block) in blocks.iter_mut().enumerate() {
        block.index = index;
    }

    blocks
}

fn parse_blocks(input: &InputSource, include_inline: bool) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let mut in_fence: Option<FenceState> = None;
    let mut last_line_no = 0usize;

    for (idx, raw_line) in input.content.lines().enumerate() {
        let line_no = idx + 1;
        last_line_no = line_no;

        if let Some(state) = &mut in_fence {
            if is_closing_fence(raw_line, state.fence_char, state.fence_len) {
                let end_line = line_no.saturating_sub(1);
                blocks.push(CodeBlock {
                    index: 0,
                    source: input.name.clone(),
                    kind: BlockKind::Fenced,
                    lang: state.lang.clone(),
                    start_line: Some(state.start_line),
                    end_line: Some(end_line),
                    code: state.buffer.trim_end_matches('\n').to_string(),
                });
                in_fence = None;
            } else {
                state.buffer.push_str(raw_line);
                state.buffer.push('\n');
            }
            continue;
        }

        if let Some((fence_char, fence_len, lang)) = parse_fence_start(raw_line) {
            in_fence = Some(FenceState {
                fence_char,
                fence_len,
                lang,
                buffer: String::new(),
                start_line: line_no + 1,
            });
            continue;
        }

        if include_inline {
            let mut inline_blocks = parse_inline_blocks(raw_line, line_no, &input.name);
            blocks.append(&mut inline_blocks);
        }
    }

    if let Some(state) = in_fence {
        // Unterminated fence; treat rest of file as the block.
        let end_line = if state.buffer.is_empty() {
            last_line_no
        } else {
            last_line_no
        };
        blocks.push(CodeBlock {
            index: 0,
            source: input.name.clone(),
            kind: BlockKind::Fenced,
            lang: state.lang,
            start_line: Some(state.start_line),
            end_line: Some(end_line),
            code: state.buffer.trim_end_matches('\n').to_string(),
        });
    }

    blocks
}

#[derive(Debug)]
struct FenceState {
    fence_char: char,
    fence_len: usize,
    lang: Option<String>,
    buffer: String,
    start_line: usize,
}

fn parse_fence_start(line: &str) -> Option<(char, usize, Option<String>)> {
    let trimmed = line.trim_start();
    let (fence_char, fence_len) = if trimmed.starts_with("```") {
        ('`', trimmed.chars().take_while(|c| *c == '`').count())
    } else if trimmed.starts_with("~~~") {
        ('~', trimmed.chars().take_while(|c| *c == '~').count())
    } else {
        return None;
    };

    let lang = trimmed
        .chars()
        .skip(fence_len)
        .collect::<String>()
        .trim()
        .to_string();
    let lang = if lang.is_empty() { None } else { Some(lang) };

    Some((fence_char, fence_len, lang))
}

fn is_closing_fence(line: &str, fence_char: char, fence_len: usize) -> bool {
    let trimmed = line.trim_start();
    let prefix_len = trimmed.chars().take_while(|c| *c == fence_char).count();
    prefix_len >= fence_len && prefix_len >= 3
}

fn parse_inline_blocks(line: &str, line_no: usize, source: &str) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let mut start_tick: Option<usize> = None;
    let mut start_idx: Option<usize> = None;
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'`' {
            let mut tick_len = 1;
            while i + tick_len < bytes.len() && bytes[i + tick_len] == b'`' {
                tick_len += 1;
            }
            if start_tick.is_none() {
                start_tick = Some(tick_len);
                start_idx = Some(i + tick_len);
            } else if let Some(open_ticks) = start_tick {
                if tick_len == open_ticks {
                    let content_start = start_idx.unwrap_or(i);
                    let content = line[content_start..i].to_string();
                    if !content.is_empty() {
                        blocks.push(CodeBlock {
                            index: 0,
                            source: source.to_string(),
                            kind: BlockKind::Inline,
                            lang: None,
                            start_line: Some(line_no),
                            end_line: Some(line_no),
                            code: content,
                        });
                    }
                    start_tick = None;
                    start_idx = None;
                }
            }
            i += tick_len;
        } else {
            i += 1;
        }
    }

    blocks
}

fn matches_lang(block: &CodeBlock, lang: &str) -> bool {
    block
        .lang
        .as_deref()
        .map(|b| b.eq_ignore_ascii_case(lang))
        .unwrap_or(false)
}

fn parse_index_filter(
    raw: Option<&str>,
) -> Result<Option<IndexFilter>, Box<dyn std::error::Error>> {
    let Some(raw) = raw else {
        return Ok(None);
    };

    if let Some((start, end)) = raw.split_once('-') {
        let start = start.trim().parse::<usize>()?;
        let end = end.trim().parse::<usize>()?;
        if start > end {
            return Err("range start must be <= end".into());
        }
        Ok(Some(IndexFilter::Range { start, end }))
    } else {
        let value = raw.trim().parse::<usize>()?;
        Ok(Some(IndexFilter::Single(value)))
    }
}

fn apply_index_filter(blocks: Vec<CodeBlock>, filter: IndexFilter) -> Vec<CodeBlock> {
    match filter {
        IndexFilter::Single(n) => blocks.into_iter().filter(|b| b.index == n).collect(),
        IndexFilter::Range { start, end } => blocks
            .into_iter()
            .filter(|b| b.index >= start && b.index <= end)
            .collect(),
    }
}

fn list_languages(blocks: &[CodeBlock]) {
    let mut langs = BTreeSet::new();
    for block in blocks {
        if let Some(lang) = &block.lang {
            langs.insert(lang.to_string());
        }
    }

    for lang in langs {
        println!("{lang}");
    }
}

fn emit_json(
    blocks: &[CodeBlock],
    include_line_numbers: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let payload: Vec<JsonBlock> = blocks
        .iter()
        .map(|b| JsonBlock {
            index: b.index,
            source: b.source.clone(),
            kind: b.kind,
            lang: b.lang.clone(),
            start_line: include_line_numbers.then_some(b.start_line).flatten(),
            end_line: include_line_numbers.then_some(b.end_line).flatten(),
            code: b.code.clone(),
        })
        .collect();

    serde_json::to_writer_pretty(io::stdout(), &payload)?;
    println!();
    Ok(())
}

#[derive(Debug, Serialize)]
struct JsonBlock {
    index: usize,
    source: String,
    kind: BlockKind,
    lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<usize>,
    code: String,
}

fn print_list(blocks: &[CodeBlock], include_line_numbers: bool) {
    for block in blocks {
        let lang = block.lang.clone().unwrap_or_else(|| "plain".to_string());
        let lines = line_count(&block.code);
        let location = if include_line_numbers {
            match (block.start_line, block.end_line) {
                (Some(start), Some(end)) if start != end => {
                    format!("{}:{}-{}", block.source, start, end)
                }
                (Some(line), _) => format!("{}:{}", block.source, line),
                _ => block.source.clone(),
            }
        } else {
            block.source.clone()
        };

        println!("{}: {} ({} lines) [{}]", block.index, lang, lines, location);
    }
}

fn print_raw(blocks: &[CodeBlock], fenced: bool, line_numbers: bool, separator: &str) {
    let rendered: Vec<String> = blocks
        .iter()
        .map(|b| render_block(b, fenced, line_numbers))
        .collect();

    print!("{}", rendered.join(separator));
    if !rendered.is_empty() && !separator.ends_with('\n') {
        println!();
    }
}

fn render_block(block: &CodeBlock, fenced: bool, line_numbers: bool) -> String {
    let mut content = if line_numbers {
        let start = block.start_line.unwrap_or(1);
        add_line_numbers(&block.code, start)
    } else {
        block.code.clone()
    };

    if fenced {
        let lang = block.lang.clone().unwrap_or_default();
        let fence = if lang.is_empty() {
            "```".to_string()
        } else {
            format!("```{}", lang)
        };
        content = format!("{fence}\n{content}\n```");
    }

    content
}

fn add_line_numbers(content: &str, start_line: usize) -> String {
    content
        .lines()
        .enumerate()
        .map(|(idx, line)| format!("{:>6}: {}", start_line + idx, line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_count(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        content.lines().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, content: &str) -> InputSource {
        InputSource {
            name: name.to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn parses_fenced_block_with_lang() {
        let blocks = collect_blocks(
            vec![input("file.md", "```rust\nfn main() {}\n```\n")],
            false,
        );
        assert_eq!(blocks.len(), 1);
        let b = &blocks[0];
        assert_eq!(b.source, "file.md");
        assert_eq!(b.kind, BlockKind::Fenced);
        assert_eq!(b.lang.as_deref(), Some("rust"));
        assert_eq!(b.code, "fn main() {}");
        assert_eq!(b.start_line, Some(2));
        assert_eq!(b.end_line, Some(2));
        assert_eq!(b.index, 0);
    }

    #[test]
    fn parses_inline_blocks_when_enabled() {
        let blocks = collect_blocks(vec![input("file.md", "a `one` b `two`")], true);
        assert_eq!(blocks.len(), 2);
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Inline));
        assert_eq!(blocks[0].code, "one");
        assert_eq!(blocks[1].code, "two");
        assert_eq!(blocks[0].start_line, Some(1));
        assert_eq!(blocks[1].start_line, Some(1));
    }

    #[test]
    fn ignores_inline_when_flag_disabled() {
        let blocks = collect_blocks(vec![input("file.md", "a `one` b `two`")], false);
        assert!(blocks.is_empty());
    }

    #[test]
    fn handles_unterminated_fence() {
        let blocks = collect_blocks(vec![input("file.md", "```js\nconsole.log('x');")], false);
        assert_eq!(blocks.len(), 1);
        let b = &blocks[0];
        assert_eq!(b.kind, BlockKind::Fenced);
        assert_eq!(b.lang.as_deref(), Some("js"));
        assert_eq!(b.start_line, Some(2));
        assert_eq!(b.end_line, Some(2));
        assert_eq!(b.code, "console.log('x');");
    }

    #[test]
    fn assigns_indices_across_sources() {
        let blocks = collect_blocks(
            vec![input("a.md", "```txt\na\n```\n"), input("b.md", "text `x`")],
            true,
        );
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].source, "a.md");
        assert_eq!(blocks[0].index, 0);
        assert_eq!(blocks[1].source, "b.md");
        assert_eq!(blocks[1].index, 1);
        assert_eq!(blocks[1].kind, BlockKind::Inline);
    }

    #[test]
    fn matches_lang_case_insensitive() {
        let block = CodeBlock {
            index: 0,
            source: "file.md".into(),
            kind: BlockKind::Fenced,
            lang: Some("Rust".into()),
            start_line: None,
            end_line: None,
            code: String::new(),
        };
        assert!(matches_lang(&block, "rust"));
        assert!(!matches_lang(&block, "python"));
    }

    #[test]
    fn parses_index_filters() {
        match parse_index_filter(Some("3")).unwrap() {
            Some(IndexFilter::Single(3)) => {}
            other => panic!("unexpected: {:?}", other),
        }

        match parse_index_filter(Some("1-4")).unwrap() {
            Some(IndexFilter::Range { start, end }) => {
                assert_eq!(start, 1);
                assert_eq!(end, 4);
            }
            other => panic!("unexpected: {:?}", other),
        }

        assert!(parse_index_filter(Some("4-2")).is_err());
    }

    #[test]
    fn renders_fenced_with_line_numbers() {
        let block = CodeBlock {
            index: 0,
            source: "file.md".into(),
            kind: BlockKind::Fenced,
            lang: Some("rs".into()),
            start_line: Some(10),
            end_line: Some(11),
            code: "fn a() {}\nfn b() {}".into(),
        };

        let rendered = render_block(&block, true, true);
        let expected = "```rs\n    10: fn a() {}\n    11: fn b() {}\n```";
        assert_eq!(rendered, expected);
    }
}

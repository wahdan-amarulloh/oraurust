//! Script splitting: script text → executable statements.
//!
//! See `GLOSSARY.md`: a [`Script`] is a buffer of SQL/PL/SQL text; statements
//! are separated by `;` (SQL) or a line-anchored `/` (PL/SQL block
//! terminator). Splitting respects string literals and comments, so neither
//! `;` inside `'...'` nor `-- ...` text can terminate a statement.

/// One executable unit extracted from a Script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// Statement text, trimmed. For `;`-terminated SQL the terminator is not
    /// included (Oracle rejects a trailing `;` on a plain SQL statement).
    pub text: String,
    /// 1-based line of the script where the statement's text begins.
    pub start_line: usize,
    /// 1-based line of the script where the statement's text ends.
    pub end_line: usize,
}

/// Splits a script into statements.
///
/// A chunk made only of whitespace and comments is not a statement.
pub fn split(script: &str) -> Vec<Statement> {
    let bytes = script.as_bytes();
    let mut statements = Vec::new();

    // Byte index where the pending statement's raw text starts.
    let mut start: Option<usize> = None;
    let mut start_line = 0usize;
    let mut end_line = 0usize;
    // Whether the pending chunk contains anything executable (not just
    // whitespace/comments).
    let mut has_code = false;
    // 1-based line counter, and whether only whitespace has been seen since
    // the start of the current line (the "/" line-anchor rule).
    let mut line = 1usize;
    let mut line_only_whitespace = true;

    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line += 1;
                line_only_whitespace = true;
                i += 1;
            }
            b' ' | b'\t' | b'\r' => i += 1,
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                // Line comment: everything to (not including) the newline.
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                // Block comment: skip to the closing `*/` (or EOF).
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            }
            b'\'' => {
                // String literal; `''` is an escaped quote.
                has_code = true;
                if start.is_none() {
                    start = Some(i);
                    start_line = line;
                }
                end_line = line;
                line_only_whitespace = false;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    if bytes[i] == b'\'' {
                        if bytes.get(i + 1) == Some(&b'\'') {
                            i += 2;
                            continue;
                        }
                        end_line = line;
                        i += 1;
                        break;
                    }
                    end_line = line;
                    i += 1;
                }
            }
            b';' => {
                // Inside a PL/SQL block (anonymous or a stored-unit
                // definition) semicolons are part of the block; only a
                // line-anchored "/" terminates it.
                let in_block = start.is_some_and(|s| is_plsql_block(&script[s..i]));
                if in_block {
                    has_code = true;
                    line_only_whitespace = false;
                    i += 1;
                    continue;
                }
                push_statement(
                    &mut statements,
                    script,
                    start.take(),
                    start_line,
                    end_line,
                    has_code,
                    i,
                );
                has_code = false;
                line_only_whitespace = false;
                i += 1;
            }
            b'/' if line_only_whitespace => {
                // Line-anchored terminator: flush the pending block and drop
                // the slash. A bare "/" with nothing pending is ignored.
                push_statement(
                    &mut statements,
                    script,
                    start.take(),
                    start_line,
                    end_line,
                    has_code,
                    i,
                );
                has_code = false;
                line_only_whitespace = false;
                i += 1;
            }
            _ => {
                has_code = true;
                if start.is_none() {
                    start = Some(i);
                    start_line = line;
                }
                end_line = line;
                line_only_whitespace = false;
                i += 1;
            }
        }
    }

    push_statement(
        &mut statements,
        script,
        start,
        start_line,
        end_line,
        has_code,
        bytes.len(),
    );
    statements
}

/// Returns true when the text pending before a `;` opens a PL/SQL block
/// whose internal semicolons must not split it: an anonymous `begin`/`declare`
/// block, or a stored-unit definition (`create [or replace] procedure|function|
/// package|trigger|type ...`).
///
/// Scans the first tokens of the chunk, skipping whitespace and comments.
fn is_plsql_block(chunk: &str) -> bool {
    const STORED_KEYWORDS: [&str; 7] = [
        "procedure", "function", "package", "trigger", "type", "library", "java",
    ];

    // Lowercased bare words in `text` — Oracle keywords are case-insensitive.
    let mut words = chunk_words(chunk).map(|w| w.to_ascii_lowercase());
    let first = words.next();
    match first.as_deref() {
        Some("begin") | Some("declare") => true,
        Some("create") => {
            // `create [or replace] [editionable|noneditionable] <unit>`
            for word in words {
                if word == "or" || word == "replace" || word == "editionable"
                    || word == "noneditionable"
                {
                    continue;
                }
                return STORED_KEYWORDS.contains(&word.as_str());
            }
            false
        }
        _ => false,
    }
}

/// Lowercased bare words in `text`, skipping whitespace, `--` line comments,
/// `/* */` comments and `'...'` literals.
fn chunk_words(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        loop {
            rest = rest.trim_start();
            if let Some(after) = rest.strip_prefix("--") {
                rest = after.split_once('\n').map_or("", |(_, tail)| tail);
            } else if let Some(after) = rest.strip_prefix("/*") {
                rest = after.split_once("*/").map_or("", |(_, tail)| tail);
            } else if let Some(after) = rest.strip_prefix('\'') {
                // Skip the literal (including `''` escapes) to its close.
                let mut chars = after.char_indices();
                let mut end = after.len();
                while let Some((idx, ch)) = chars.next() {
                    if ch == '\'' {
                        let next_is_quote = after[idx + 1..].starts_with('\'');
                        if next_is_quote {
                            chars.next();
                        } else {
                            end = idx + 1;
                            break;
                        }
                    }
                }
                rest = &after[end..];
            } else if rest.is_empty() {
                return None;
            } else {
                let end = rest
                    .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$' || c == '#'))
                    .unwrap_or(rest.len());
                if end == 0 {
                    // Punctuation we don't care about (e.g. `(`, `=`). Skip a
                    // whole char, not a byte — the char may be multi-byte.
                    let skip = rest.chars().next().map_or(1, char::len_utf8);
                    rest = &rest[skip..];
                    continue;
                }
                let (word, tail) = rest.split_at(end);
                rest = tail;
                return Some(word);
            }
        }
    })
}

/// Emits the pending chunk as a statement if it contains code.
fn push_statement(
    statements: &mut Vec<Statement>,
    script: &str,
    start: Option<usize>,
    start_line: usize,
    end_line: usize,
    has_code: bool,
    end: usize,
) {
    let Some(start) = start else { return };
    if !has_code {
        return;
    }
    let text = script[start..end].trim();
    if !text.is_empty() {
        statements.push(Statement {
            text: text.to_string(),
            start_line,
            end_line,
        });
    }
}

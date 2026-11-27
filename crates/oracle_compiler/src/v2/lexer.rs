//! Span-tracked lexer for the `v2` compositional Oracle parser.
//!
//! Converts normalized MTG Oracle text into a flat slice of typed [`Token`] values carrying
//! exact byte [`Span`]s: mana symbols (`{1}`, `{R}`, `{T}`, `{E}`), P/T values (`2/2`, `0/0`),
//! P/T modifiers (`+2/+1`, `-1/-1`, `+X/+0`), integers, quoted strings, punctuation, and words.

use crate::v2::ir::{ManaSymbol, ParseError, ParseStage, PtDelta, Span};

/// One lexed token with its exact byte span in the source slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind<'a>,
    pub span: Span,
}

/// Typed token variants emitted by [`tokenize`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind<'a> {
    /// Mana or special symbol enclosed in braces (`{W}`, `{2}`, `{T}`, `{E}`, `{W/U}`).
    Mana(ManaSymbol),
    /// Signed power/toughness modifier (`+2/+1`, `-1/-1`, `+X/+0`).
    PtMod(PtDelta, PtDelta),
    /// Base power/toughness pair (`2/2`, `0/0`, `1/1`, `*/\*`).
    BasePt(i32, i32),
    /// Unsigned integer literal (`0`, `1`, `42`).
    Number(i32),
    /// Double-quoted embedded text (inner slice without surrounding quotes).
    Quoted(&'a str),
    /// Self-reference tilde `~`.
    Tilde,
    /// Word token (letters, digits, apostrophes, internal hyphens, and `and/or`).
    Word(&'a str),
    /// Punctuation marks.
    Comma,
    Period,
    Colon,
    Semicolon,
    EmDash,
    Bullet,
    Plus,
    Minus,
    Slash,
    Bang,
    Pipe,
}

/// Tokenize normalized MTG Oracle text into a vector of span-tracked [`Token`]s.
///
/// # Errors
/// Returns [`ParseError`] on unterminated double quotes, malformed `{...}` mana symbols,
/// or unexpected non-MTG punctuation.
pub fn tokenize(input: &str) -> Result<Vec<Token<'_>>, ParseError> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::with_capacity(input.len() / 4 + 4);
    let mut i = 0usize;

    while i < bytes.len() {
        let ch = input[i..].chars().next().expect("valid utf8 boundary");
        if ch.is_whitespace() {
            i += ch.len_utf8();
            continue;
        }

        let start = i;

        // 1. Mana / special symbol `{...}`
        if ch == '{' {
            let Some(rel_close) = input[i + 1..].find('}') else {
                return Err(ParseError {
                    span: Span {
                        start,
                        end: input.len(),
                    },
                    stage: ParseStage::Lexer,
                    message: "unterminated mana symbol `{`".to_string(),
                    text: input[start..].to_string(),
                });
            };
            let inner = &input[i + 1..i + 1 + rel_close];
            let end = i + 2 + rel_close;
            let sym = parse_mana_symbol(inner).ok_or_else(|| ParseError {
                span: Span { start, end },
                stage: ParseStage::Lexer,
                message: format!("unrecognized mana symbol `{{{inner}}}`"),
                text: input[start..end].to_string(),
            })?;
            tokens.push(Token {
                kind: TokenKind::Mana(sym),
                span: Span { start, end },
            });
            i = end;
            continue;
        }

        // 2. Quoted ability / text `"..."` (or nested single-quoted `'...'` at token start,
        //    excluding possessives/contractions like `~'s` or `'t`).
        let is_open_single_quote = ch == '\''
            && (start == 0 || bytes[start - 1].is_ascii_whitespace())
            && !input[start..].starts_with("'s ")
            && !input[start..].starts_with("'s.");
        if ch == '"' || is_open_single_quote {
            let quote = ch;
            let mut close_idx = None;
            let mut scan = i + 1;
            while scan < bytes.len() {
                if bytes[scan] == quote as u8 {
                    let next_byte = bytes.get(scan + 1).copied();
                    let is_boundary = quote == '"'
                        || next_byte.is_none()
                        || next_byte.is_some_and(|b| {
                            b.is_ascii_whitespace() || matches!(b, b'.' | b',' | b';' | b')')
                        });
                    if is_boundary {
                        close_idx = Some(scan);
                        break;
                    }
                }
                scan += 1;
            }
            let Some(abs_close) = close_idx else {
                return Err(ParseError {
                    span: Span {
                        start,
                        end: input.len(),
                    },
                    stage: ParseStage::Lexer,
                    message: "unterminated quoted string".to_string(),
                    text: input[start..].to_string(),
                });
            };
            let inner = &input[i + 1..abs_close];
            let end = abs_close + 1;
            tokens.push(Token {
                kind: TokenKind::Quoted(inner),
                span: Span { start, end },
            });
            i = end;
            continue;
        }

        // 3. Single-char punctuation
        match ch {
            '~' => {
                // Keep `~'s` as tilde + possessive word suffix handled cleanly, or lex `~` alone.
                tokens.push(Token {
                    kind: TokenKind::Tilde,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
                continue;
            }
            ',' => {
                tokens.push(Token {
                    kind: TokenKind::Comma,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
                continue;
            }
            '.' => {
                tokens.push(Token {
                    kind: TokenKind::Period,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
                continue;
            }
            ':' => {
                tokens.push(Token {
                    kind: TokenKind::Colon,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
                continue;
            }
            ';' => {
                tokens.push(Token {
                    kind: TokenKind::Semicolon,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
                continue;
            }
            '\u{2014}' => {
                let end = i + ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::EmDash,
                    span: Span { start, end },
                });
                i = end;
                continue;
            }
            '\u{2022}' => {
                let end = i + ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Bullet,
                    span: Span { start, end },
                });
                i = end;
                continue;
            }
            '!' => {
                tokens.push(Token {
                    kind: TokenKind::Bang,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
                continue;
            }
            '|' => {
                tokens.push(Token {
                    kind: TokenKind::Pipe,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
                continue;
            }
            '\u{221E}' => {
                let end = i + ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Word("infinity"),
                    span: Span { start, end },
                });
                i = end;
                continue;
            }
            '&' => {
                tokens.push(Token {
                    kind: TokenKind::Word("and"),
                    span: Span { start, end: i + 1 },
                });
                i += 1;
                continue;
            }
            _ => {}
        }

        // 4. Signed P/T modifier (`+2/+1`, `-1/-1`, `+X/+0`) or bare `+`/`-`
        if ch == '+' || ch == '-' || ch == '\u{2212}' {
            if let Some((dx, dy, end)) = scan_pt_mod(input, i) {
                tokens.push(Token {
                    kind: TokenKind::PtMod(dx, dy),
                    span: Span { start, end },
                });
                i = end;
                continue;
            }
            let end = i + ch.len_utf8();
            tokens.push(Token {
                kind: if ch == '+' {
                    TokenKind::Plus
                } else {
                    TokenKind::Minus
                },
                span: Span { start, end },
            });
            i = end;
            continue;
        }

        // 5. Base P/T (`2/2`, `0/0`) or unsigned integer (`3`, `10`)
        if ch.is_ascii_digit() {
            let mut end = i;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end < bytes.len()
                && bytes[end] == b'/'
                && end + 1 < bytes.len()
                && bytes[end + 1].is_ascii_digit()
            {
                let mut end2 = end + 1;
                while end2 < bytes.len() && bytes[end2].is_ascii_digit() {
                    end2 += 1;
                }
                let p = input[start..end].parse::<i32>().map_err(|_| ParseError {
                    span: Span { start, end: end2 },
                    stage: ParseStage::Lexer,
                    message: "power integer overflow".to_string(),
                    text: input[start..end2].to_string(),
                })?;
                let t = input[end + 1..end2]
                    .parse::<i32>()
                    .map_err(|_| ParseError {
                        span: Span { start, end: end2 },
                        stage: ParseStage::Lexer,
                        message: "toughness integer overflow".to_string(),
                        text: input[start..end2].to_string(),
                    })?;
                tokens.push(Token {
                    kind: TokenKind::BasePt(p, t),
                    span: Span { start, end: end2 },
                });
                i = end2;
                continue;
            }
            let n = input[start..end].parse::<i32>().map_err(|_| ParseError {
                span: Span { start, end },
                stage: ParseStage::Lexer,
                message: "integer literal overflow".to_string(),
                text: input[start..end].to_string(),
            })?;
            tokens.push(Token {
                kind: TokenKind::Number(n),
                span: Span { start, end },
            });
            i = end;
            continue;
        }

        if ch == '/' {
            tokens.push(Token {
                kind: TokenKind::Slash,
                span: Span { start, end: i + 1 },
            });
            i += 1;
            continue;
        }

        // 6. Word (including apostrophes like `owner's`, `can't`, `you've`, `owners'`,
        //    internal hyphens like `face-down`, `non-human`, and `and/or`).
        if ch.is_alphabetic() || ch == '\'' {
            let mut end = i;
            while end < bytes.len() {
                let c = input[end..].chars().next().expect("utf8");
                if c.is_alphanumeric() || c == '\'' || c == '\u{2019}' {
                    end += c.len_utf8();
                } else if c == '-'
                    && input[end + 1..]
                        .chars()
                        .next()
                        .is_some_and(char::is_alphanumeric)
                {
                    end += 1;
                } else if c == '/'
                    && input[start..end].eq_ignore_ascii_case("and")
                    && input[end + 1..].to_ascii_lowercase().starts_with("or")
                {
                    let after_or = end + 3;
                    let next_ok = input[after_or..]
                        .chars()
                        .next()
                        .is_none_or(|nc| !nc.is_alphanumeric());
                    if next_ok {
                        end = after_or;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            tokens.push(Token {
                kind: TokenKind::Word(&input[start..end]),
                span: Span { start, end },
            });
            i = end;
            continue;
        }

        return Err(ParseError {
            span: Span {
                start,
                end: start + ch.len_utf8(),
            },
            stage: ParseStage::Lexer,
            message: format!("unexpected character `{ch}`"),
            text: input[start..start + ch.len_utf8()].to_string(),
        });
    }

    Ok(tokens)
}

fn scan_pt_mod(input: &str, start: usize) -> Option<(PtDelta, PtDelta, usize)> {
    let (dx, after_x) = scan_single_pt_delta(input, start)?;
    if input.as_bytes().get(after_x) != Some(&b'/') {
        return None;
    }
    let (dy, end) = scan_single_pt_delta(input, after_x + 1)?;
    // Ensure token boundary after dy
    if input[end..]
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '/')
    {
        return None;
    }
    Some((dx, dy, end))
}

fn scan_single_pt_delta(input: &str, start: usize) -> Option<(PtDelta, usize)> {
    let ch = input[start..].chars().next()?;
    let negative = match ch {
        '+' => false,
        '-' | '\u{2212}' => true,
        _ => return None,
    };
    let pos = start + ch.len_utf8();
    let rest = &input[pos..];
    if let Some(first) = rest.chars().next()
        && first.eq_ignore_ascii_case(&'x')
    {
        let delta = if negative {
            PtDelta::MinusX
        } else {
            PtDelta::PlusX
        };
        return Some((delta, pos + 1));
    }
    let mut end = pos;
    while end < input.len() && input.as_bytes()[end].is_ascii_digit() {
        end += 1;
    }
    if end == pos {
        return None;
    }
    let mag: i16 = input[pos..end].parse().ok()?;
    let val = if negative { -mag } else { mag };
    Some((PtDelta::Fixed(val), end))
}

/// Parse the inner text of a `{...}` symbol token into a typed [`ManaSymbol`].
#[must_use]
pub fn parse_mana_symbol(inner: &str) -> Option<ManaSymbol> {
    let up = inner.trim().to_ascii_uppercase();
    if let Some((a, b)) = up.split_once('/') {
        if b == "P" {
            return Some(ManaSymbol::Phyrexian(Box::new(parse_mana_symbol(a)?)));
        }
        return Some(ManaSymbol::Hybrid(
            Box::new(parse_mana_symbol(a)?),
            Box::new(parse_mana_symbol(b)?),
        ));
    }
    match up.as_str() {
        "W" => Some(ManaSymbol::White),
        "U" => Some(ManaSymbol::Blue),
        "B" => Some(ManaSymbol::Black),
        "R" => Some(ManaSymbol::Red),
        "G" => Some(ManaSymbol::Green),
        "C" => Some(ManaSymbol::Colorless),
        "S" => Some(ManaSymbol::Snow),
        "X" => Some(ManaSymbol::VariableX),
        "T" => Some(ManaSymbol::Tap),
        "Q" => Some(ManaSymbol::Untap),
        "E" => Some(ManaSymbol::Energy),
        "TK" => Some(ManaSymbol::Ticket),
        "P" => Some(ManaSymbol::Pawprint),
        other => other.parse::<u16>().ok().map(ManaSymbol::Generic),
    }
}

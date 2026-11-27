//! Composable token-stream parser combinators for `v2`.
//!
//! Provides transactional checkpointing, furthest-error tracking, strict `expect_eof`
//! enforcement, and generic combinators (`opt`, `many0`, `sep_by1`, `alt2`, `alt3`) over
//! [`TokenStream`].

use crate::v2::ir::{ManaCost, ManaSymbol, ParseError, ParseStage, PtDelta, Span};
use crate::v2::lexer::{Token, TokenKind};
use crate::vocab::{Vocabulary, parse_number_word};

/// Result type for all `v2` parser combinators.
pub type PResult<T> = Result<T, ParseError>;

/// Cursor over a slice of lexed [`Token`]s with transaction checkpoints and source-slice access.
#[derive(Clone)]
pub struct TokenStream<'a> {
    pub source: &'a str,
    pub tokens: &'a [Token<'a>],
    pub pos: usize,
    pub vocab: &'a Vocabulary,
    pub stage: ParseStage,
}

/// Saved stream position for transactional backtracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint(usize);

impl<'a> TokenStream<'a> {
    /// Construct a fresh stream at token 0.
    #[must_use]
    pub fn new(
        source: &'a str,
        tokens: &'a [Token<'a>],
        vocab: &'a Vocabulary,
        stage: ParseStage,
    ) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            vocab,
            stage,
        }
    }

    /// Save current cursor position.
    #[must_use]
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint(self.pos)
    }

    /// Restore a previously saved cursor position.
    pub fn restore(&mut self, cp: Checkpoint) {
        self.pos = cp.0;
    }

    /// Remaining unconsumed tokens count.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.tokens.len().saturating_sub(self.pos)
    }

    /// True when all tokens have been consumed.
    #[must_use]
    pub fn is_eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Peek at the next token without advancing.
    #[must_use]
    pub fn peek(&self) -> Option<&Token<'a>> {
        self.tokens.get(self.pos)
    }

    /// Peek `offset` tokens ahead without advancing.
    #[must_use]
    pub fn peek_n(&self, offset: usize) -> Option<&Token<'a>> {
        self.tokens.get(self.pos + offset)
    }

    /// Consume and return the next token.
    pub fn next_tok(&mut self) -> Option<&Token<'a>> {
        let tok = self.tokens.get(self.pos)?;
        self.pos += 1;
        Some(tok)
    }

    /// Byte span of the current token, or end-of-source span at EOF.
    #[must_use]
    pub fn current_span(&self) -> Span {
        if let Some(t) = self.peek() {
            t.span
        } else {
            Span {
                start: self.source.len(),
                end: self.source.len(),
            }
        }
    }

    /// Build a [`ParseError`] anchored at the current cursor position.
    #[must_use]
    pub fn err(&self, message: impl Into<String>) -> ParseError {
        let span = self.current_span();
        ParseError {
            span,
            stage: self.stage,
            message: message.into(),
            text: self
                .source
                .get(span.start..)
                .unwrap_or(self.source)
                .to_string(),
        }
    }

    /// Enforce that zero unconsumed tokens remain (fail-closed end-of-clause check).
    ///
    /// # Errors
    /// Returns [`ParseError`] if any trailing token remains unparsed.
    pub fn expect_eof(&self) -> PResult<()> {
        if self.is_eof() {
            Ok(())
        } else {
            Err(self.err("unexpected trailing unconsumed tokens"))
        }
    }

    /// Consume a specific word case-insensitively.
    pub fn eat_word(&mut self, expected: &str) -> bool {
        if let Some(Token {
            kind: TokenKind::Word(w),
            ..
        }) = self.peek()
            && w.eq_ignore_ascii_case(expected)
        {
            self.pos += 1;
            return true;
        }
        false
    }

    /// Consume an exact sequence of words case-insensitively; restores cursor if any word fails.
    pub fn eat_phrase(&mut self, words: &[&str]) -> bool {
        let cp = self.checkpoint();
        for &w in words {
            if !self.eat_word(w) {
                self.restore(cp);
                return false;
            }
        }
        true
    }

    /// Peek if the next tokens match an exact sequence of words case-insensitively.
    #[must_use]
    pub fn peek_phrase(&self, words: &[&str]) -> bool {
        for (idx, &expected) in words.iter().enumerate() {
            match self.peek_n(idx) {
                Some(Token {
                    kind: TokenKind::Word(w),
                    ..
                }) if w.eq_ignore_ascii_case(expected) => {}
                _ => return false,
            }
        }
        true
    }

    /// Peek the lowercase word at offset 0 if it is a `TokenKind::Word`.
    #[must_use]
    pub fn peek_word(&self) -> Option<&'a str> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::Word(w),
                ..
            }) => Some(*w),
            _ => None,
        }
    }

    /// Consume any `TokenKind::Word` and return its source slice.
    pub fn next_word(&mut self) -> Option<&'a str> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::Word(w),
                ..
            }) => {
                let s = *w;
                self.pos += 1;
                Some(s)
            }
            _ => None,
        }
    }

    /// Consume a specific punctuation token kind.
    pub fn eat_comma(&mut self) -> bool {
        if matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Comma)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub fn eat_period(&mut self) -> bool {
        if matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Period)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub fn eat_colon(&mut self) -> bool {
        if matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Colon)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub fn eat_em_dash(&mut self) -> bool {
        if matches!(self.peek().map(|t| &t.kind), Some(TokenKind::EmDash)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub fn eat_tilde(&mut self) -> bool {
        if matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Tilde)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Consume a single mana symbol token.
    pub fn next_mana(&mut self) -> Option<ManaSymbol> {
        if let Some(Token {
            kind: TokenKind::Mana(sym),
            ..
        }) = self.peek()
        {
            let cloned = sym.clone();
            self.pos += 1;
            Some(cloned)
        } else {
            None
        }
    }

    /// Consume one or more consecutive mana symbols (excluding `{T}` / `{Q}` unless caller wants them).
    pub fn eat_mana_cost(&mut self) -> Option<ManaCost> {
        let mut symbols = Vec::new();
        while let Some(Token {
            kind: TokenKind::Mana(sym),
            ..
        }) = self.peek()
        {
            if matches!(sym, ManaSymbol::Tap | ManaSymbol::Untap) {
                break;
            }
            symbols.push(sym.clone());
            self.pos += 1;
        }
        if symbols.is_empty() {
            None
        } else {
            Some(ManaCost { symbols })
        }
    }

    /// Consume a P/T modifier (`+2/+1`, `-1/-1`, `+X/+0`).
    pub fn next_pt_mod(&mut self) -> Option<(PtDelta, PtDelta)> {
        if let Some(Token {
            kind: TokenKind::PtMod(dx, dy),
            ..
        }) = self.peek()
        {
            let pair = (*dx, *dy);
            self.pos += 1;
            Some(pair)
        } else {
            None
        }
    }

    /// Consume a base P/T pair (`2/2`, `0/0`).
    pub fn next_base_pt(&mut self) -> Option<(i32, i32)> {
        if let Some(Token {
            kind: TokenKind::BasePt(p, t),
            ..
        }) = self.peek()
        {
            let pair = (*p, *t);
            self.pos += 1;
            Some(pair)
        } else {
            None
        }
    }

    /// Consume an integer literal or English number word (`one`..`twenty`, `a`/`an` when `allow_article`).
    pub fn next_number(&mut self, allow_article: bool) -> Option<i32> {
        match self.peek().map(|t| &t.kind) {
            Some(TokenKind::Number(n)) => {
                let v = *n;
                self.pos += 1;
                Some(v)
            }
            Some(TokenKind::Word(w)) => {
                let low = w.to_ascii_lowercase();
                if allow_article && (low == "a" || low == "an") {
                    self.pos += 1;
                    return Some(1);
                }
                let n = parse_number_word(&low)?;
                self.pos += 1;
                Some(n)
            }
            _ => None,
        }
    }

    /// Consume a double-quoted string token and return its inner text.
    pub fn next_quoted(&mut self) -> Option<&'a str> {
        if let Some(Token {
            kind: TokenKind::Quoted(inner),
            ..
        }) = self.peek()
        {
            let s = *inner;
            self.pos += 1;
            Some(s)
        } else {
            None
        }
    }

    /// Create a sub-stream scoped to a slice of tokens with a specific stage.
    #[must_use]
    pub fn sub_stream(&self, slice: &'a [Token<'a>], stage: ParseStage) -> Self {
        Self {
            source: self.source,
            tokens: slice,
            pos: 0,
            vocab: self.vocab,
            stage,
        }
    }
}

/// Run a parser transactionally: if it returns `Err`, restore the cursor to its prior position.
pub fn try_parse<'a, T, F>(ts: &mut TokenStream<'a>, f: F) -> PResult<T>
where
    F: FnOnce(&mut TokenStream<'a>) -> PResult<T>,
{
    let cp = ts.checkpoint();
    match f(ts) {
        Ok(val) => Ok(val),
        Err(err) => {
            ts.restore(cp);
            Err(err)
        }
    }
}

/// Optional parser combinator: returns `Some(T)` on success, restores cursor and returns `None` on failure.
pub fn opt<'a, T, F>(ts: &mut TokenStream<'a>, f: F) -> Option<T>
where
    F: FnOnce(&mut TokenStream<'a>) -> PResult<T>,
{
    try_parse(ts, f).ok()
}

/// Ordered choice across parser functions, tracking the furthest-reaching error on total failure.
pub fn alt<'a, T>(
    ts: &mut TokenStream<'a>,
    parsers: &[fn(&mut TokenStream<'a>) -> PResult<T>],
) -> PResult<T> {
    let mut best_err: Option<ParseError> = None;
    for &p in parsers {
        match try_parse(ts, p) {
            Ok(v) => return Ok(v),
            Err(e) => {
                let replace = best_err
                    .as_ref()
                    .is_none_or(|prev| e.span.start >= prev.span.start);
                if replace {
                    best_err = Some(e);
                }
            }
        }
    }
    Err(best_err.unwrap_or_else(|| ts.err("no matching alternative")))
}

/// Parse one or more items separated by a separator parser.
pub fn sep_by1<'a, T, S, F, Sep>(
    ts: &mut TokenStream<'a>,
    mut item: F,
    mut sep: Sep,
) -> PResult<Vec<T>>
where
    F: FnMut(&mut TokenStream<'a>) -> PResult<T>,
    Sep: FnMut(&mut TokenStream<'a>) -> PResult<S>,
{
    let mut out = vec![item(ts)?];
    loop {
        let cp = ts.checkpoint();
        if sep(ts).is_ok() {
            match item(ts) {
                Ok(next_item) => out.push(next_item),
                Err(_) => {
                    ts.restore(cp);
                    break;
                }
            }
        } else {
            ts.restore(cp);
            break;
        }
    }
    Ok(out)
}

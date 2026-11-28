//! Standalone zero-unsafe compositional Oracle Parser (`v2`) for Spellwake.
//!
//! Compiles Magic Oracle text into strictly-typed, fail-closed `v2::ir` ASTs ready for direct
//! lowering into `spellwake-engine`'s Universal Rules Toolbox.

pub mod ast;
pub mod preprocess;
pub mod standardize;
pub mod v2;
pub mod vocab;

use std::error::Error;
use std::fmt;

pub use v2::{
    CompiledCardV2, CompiledFaceV2, ParseError, ParseStage, Span, V2CorpusSummary, compile_card_v2,
    compile_face_v2,
};
pub use vocab::Vocabulary;

/// Typed library error for vocabulary loading and I/O failures.
#[derive(Debug)]
pub enum CompilerError {
    /// Underlying filesystem error.
    Io(std::io::Error),
    /// Underlying JSON deserialization error.
    Json(serde_json::Error),
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "i/o failure reading compiler input: {err}"),
            Self::Json(err) => write!(f, "json parse failure reading compiler input: {err}"),
        }
    }
}

impl Error for CompilerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Json(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for CompilerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for CompilerError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

/// Borrowed face input for compilation.
#[derive(Debug, Clone, Copy)]
pub struct FaceInput<'a> {
    pub name: &'a str,
    pub type_line: &'a str,
    pub oracle_text: &'a str,
}

/// Borrowed card input for compilation.
#[derive(Debug, Clone)]
pub struct CardInput<'a> {
    pub oracle_id: &'a str,
    pub name: &'a str,
    pub layout: &'a str,
    pub faces: Vec<FaceInput<'a>>,
}

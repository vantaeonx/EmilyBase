//! Original bounded SQL lexer, AST and parser. Execution is a separate next increment.
pub mod ast;
mod lexer;
mod parser;
pub use parser::parse;

pub const MAX_SQL_BYTES: usize = 16_384;
pub const MAX_TOKENS: usize = 4096;
pub const MAX_STATEMENTS: usize = 64;
pub const MAX_PARAMETERS: usize = 256;
pub const MAX_EXPRESSION_DEPTH: usize = 32;
pub const MAX_INSERT_ROWS: usize = 256;
pub const MAX_RESULT_ROWS: usize = 10_000;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("SQL syntax error at byte {offset}: expected {expected}")]
    Syntax {
        offset: usize,
        expected: &'static str,
    },
    #[error("SQL limit exceeded: {0}")]
    Limit(&'static str),
    #[error("invalid SQL literal at byte {0}")]
    Literal(usize),
    #[error("invalid SQL schema")]
    Schema,
}

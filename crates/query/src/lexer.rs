use crate::{Error, MAX_PARAMETERS, MAX_SQL_BYTES, MAX_TOKENS, Result};
use emilybase_catalog::MAX_VALUE_BYTES;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    Word(String),
    Identifier(String),
    String(String),
    Number(String),
    Parameter(usize),
    Symbol(char),
    Operator(&'static str),
    End,
}

#[derive(Debug, Clone)]
pub(crate) struct Token {
    pub kind: Kind,
    pub offset: usize,
}

pub(crate) fn lex(sql: &str) -> Result<Vec<Token>> {
    if sql.len() > MAX_SQL_BYTES {
        return Err(Error::Limit("input bytes"));
    }
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let offset = i;
        let byte = bytes[i];
        if byte.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"--") {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            i += 2;
            while i + 1 < bytes.len() && !bytes[i..].starts_with(b"*/") {
                i += 1;
            }
            if i + 1 >= bytes.len() {
                return Err(syntax(offset, "closing comment"));
            }
            i += 2;
            continue;
        }
        if tokens.len() >= MAX_TOKENS {
            return Err(Error::Limit("tokens"));
        }
        let kind = if byte == b'\'' || byte == b'"' {
            i += 1;
            let mut value = Vec::new();
            loop {
                if i >= bytes.len() {
                    return Err(syntax(offset, "closing quote"));
                }
                if bytes[i] == byte {
                    i += 1;
                    if i < bytes.len() && bytes[i] == byte {
                        value.push(byte);
                        i += 1;
                    } else {
                        break;
                    }
                } else {
                    value.push(bytes[i]);
                    i += 1;
                }
                if value.len() > MAX_VALUE_BYTES {
                    return Err(Error::Limit("literal bytes"));
                }
            }
            let text = String::from_utf8(value).map_err(|_| Error::Literal(offset))?;
            if byte == b'\'' {
                Kind::String(text)
            } else {
                Kind::Identifier(text)
            }
        } else if byte.is_ascii_alphabetic() || byte == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            Kind::Word(sql[offset..i].to_owned())
        } else if byte.is_ascii_digit() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
                i += 1;
                if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
                    i += 1;
                }
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i == start {
                    return Err(Error::Literal(offset));
                }
            }
            Kind::Number(sql[offset..i].to_owned())
        } else if byte == b'$' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let number: usize = sql[start..i].parse().map_err(|_| Error::Literal(offset))?;
            if number == 0 || number > MAX_PARAMETERS {
                return Err(Error::Limit("parameter position"));
            }
            Kind::Parameter(number)
        } else {
            i += 1;
            match byte {
                b'(' | b')' | b',' | b';' | b'.' | b'*' | b'+' | b'-' => Kind::Symbol(byte as char),
                b'=' => Kind::Operator("="),
                b'<' | b'>' | b'!' => {
                    let second = bytes.get(i).copied();
                    let op = match (byte, second) {
                        (b'<', Some(b'=')) => "<=",
                        (b'>', Some(b'=')) => ">=",
                        (b'<', Some(b'>')) => "<>",
                        (b'!', Some(b'=')) => "!=",
                        (b'<', _) => "<",
                        (b'>', _) => ">",
                        _ => return Err(syntax(offset, "comparison operator")),
                    };
                    if op.len() == 2 {
                        i += 1;
                    }
                    Kind::Operator(op)
                }
                _ => return Err(syntax(offset, "SQL token")),
            }
        };
        tokens.push(Token { kind, offset });
    }
    tokens.push(Token {
        kind: Kind::End,
        offset: sql.len(),
    });
    Ok(tokens)
}

pub(crate) fn syntax(offset: usize, expected: &'static str) -> Error {
    Error::Syntax { offset, expected }
}

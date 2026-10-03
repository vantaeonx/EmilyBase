use crate::ast::*;
use crate::lexer::{Kind, Token, lex, syntax};
use crate::{
    Error, MAX_EXPRESSION_DEPTH, MAX_INSERT_ROWS, MAX_RESULT_ROWS, MAX_STATEMENTS, Result,
};
use emilybase_catalog::{Column, DataType, MAX_COLUMNS, MAX_NAME_BYTES, Schema, Value};

pub fn parse(sql: &str) -> Result<Vec<Statement>> {
    let mut parser = Parser {
        tokens: lex(sql)?,
        position: 0,
    };
    let mut statements = Vec::new();
    while parser.peek() != &Kind::End {
        if statements.len() >= MAX_STATEMENTS {
            return Err(Error::Limit("statements"));
        }
        statements.push(parser.statement()?);
        if !parser.symbol(';') && parser.peek() != &Kind::End {
            return Err(parser.error("semicolon"));
        }
    }
    if statements.is_empty() {
        return Err(parser.error("statement"));
    }
    Ok(statements)
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}
impl Parser {
    fn peek(&self) -> &Kind {
        &self.tokens[self.position].kind
    }
    fn error(&self, expected: &'static str) -> Error {
        syntax(self.tokens[self.position].offset, expected)
    }
    fn next(&mut self) -> Kind {
        let kind = self.peek().clone();
        if kind != Kind::End {
            self.position += 1;
        }
        kind
    }
    fn word(&mut self, expected: &str) -> bool {
        if matches!(self.peek(),Kind::Word(text) if text.eq_ignore_ascii_case(expected)) {
            self.next();
            true
        } else {
            false
        }
    }
    fn require(&mut self, word: &'static str) -> Result<()> {
        if self.word(word) {
            Ok(())
        } else {
            Err(self.error(word))
        }
    }
    fn symbol(&mut self, expected: char) -> bool {
        if self.peek() == &Kind::Symbol(expected) {
            self.next();
            true
        } else {
            false
        }
    }
    fn punctuation(&mut self, expected: char) -> Result<()> {
        if self.symbol(expected) {
            Ok(())
        } else {
            Err(self.error("punctuation"))
        }
    }
    fn identifier(&mut self) -> Result<String> {
        let name = match self.peek() {
            Kind::Word(s) | Kind::Identifier(s) => s.clone(),
            _ => return Err(self.error("identifier")),
        };
        if name.is_empty()
            || name.len() > MAX_NAME_BYTES
            || !name
                .as_bytes()
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
            || !(name.as_bytes()[0].is_ascii_alphabetic() || name.as_bytes()[0] == b'_')
        {
            return Err(self.error("ASCII identifier of at most 63 bytes"));
        }
        self.next();
        Ok(name)
    }
    fn names(&mut self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        loop {
            if names.len() >= MAX_COLUMNS {
                return Err(Error::Limit("columns"));
            }
            names.push(self.identifier()?);
            if !self.symbol(',') {
                break;
            }
        }
        Ok(names)
    }
    fn statement(&mut self) -> Result<Statement> {
        if self.word("CREATE") {
            self.require("TABLE")?;
            return self.create();
        }
        if self.word("DROP") {
            self.require("TABLE")?;
            return Ok(Statement::Drop(self.identifier()?));
        }
        if self.word("INSERT") {
            return self.insert();
        }
        if self.word("SELECT") {
            return Ok(Statement::Select(Box::new(self.select()?)));
        }
        if self.word("UPDATE") {
            return self.update();
        }
        if self.word("DELETE") {
            self.require("FROM")?;
            let table = self.identifier()?;
            return Ok(Statement::Delete {
                table,
                filter: self.filter()?,
            });
        }
        if self.word("BEGIN") {
            self.word("TRANSACTION");
            return Ok(Statement::Begin);
        }
        if self.word("COMMIT") {
            return Ok(Statement::Commit);
        }
        if self.word("ROLLBACK") {
            return Ok(Statement::Rollback);
        }
        Err(self.error("supported statement"))
    }
    fn create(&mut self) -> Result<Statement> {
        let name = self.identifier()?;
        self.punctuation('(')?;
        let mut columns = Vec::new();
        let mut primary = None;
        loop {
            if self.word("PRIMARY") {
                self.require("KEY")?;
                self.punctuation('(')?;
                let key = self.identifier()?;
                self.punctuation(')')?;
                if primary.replace(key).is_some() {
                    return Err(Error::Schema);
                }
            } else {
                if columns.len() >= MAX_COLUMNS {
                    return Err(Error::Limit("columns"));
                }
                let name = self.identifier()?;
                let kind = if self.word("INTEGER") || self.word("INT") || self.word("BIGINT") {
                    DataType::Integer
                } else if self.word("TEXT") {
                    DataType::Text
                } else if self.word("BOOLEAN") || self.word("BOOL") {
                    DataType::Boolean
                } else if self.word("FLOAT") || self.word("REAL") || self.word("DOUBLE") {
                    DataType::Float
                } else if self.word("BYTES") || self.word("BLOB") {
                    DataType::Bytes
                } else {
                    return Err(self.error("supported type"));
                };
                let mut nullable = true;
                if self.word("NOT") {
                    self.require("NULL")?;
                    nullable = false;
                }
                if self.word("PRIMARY") {
                    self.require("KEY")?;
                    nullable = false;
                    if primary.replace(name.clone()).is_some() {
                        return Err(Error::Schema);
                    }
                }
                columns.push(Column {
                    name,
                    data_type: kind,
                    nullable,
                });
            }
            if !self.symbol(',') {
                break;
            }
        }
        self.punctuation(')')?;
        let primary = primary.ok_or(Error::Schema)?;
        let position = columns
            .iter()
            .position(|c| c.name == primary)
            .ok_or(Error::Schema)?;
        columns[position].nullable = false;
        let schema = Schema {
            name,
            columns,
            primary_key: position as u16,
        };
        schema.validate().map_err(|_| Error::Schema)?;
        Ok(Statement::Create(schema))
    }
    fn scalar(&mut self) -> Result<Scalar> {
        let offset = self.tokens[self.position].offset;
        if let Kind::Parameter(n) = self.peek() {
            let n = *n;
            self.next();
            return Ok(Scalar::Parameter(n));
        }
        let value = if self.word("NULL") {
            Value::Null
        } else if self.word("TRUE") {
            Value::Boolean(true)
        } else if self.word("FALSE") {
            Value::Boolean(false)
        } else if self.word("X") {
            let Kind::String(hex) = self.next() else {
                return Err(self.error("hex string"));
            };
            if hex.len() % 2 != 0 {
                return Err(Error::Literal(offset));
            }
            let mut bytes = Vec::new();
            for pair in hex.as_bytes().as_chunks::<2>().0 {
                let digit = |b: u8| (b as char).to_digit(16).map(|v| v as u8);
                bytes.push(
                    digit(pair[0]).ok_or(Error::Literal(offset))? * 16
                        + digit(pair[1]).ok_or(Error::Literal(offset))?,
                );
            }
            Value::Bytes(bytes)
        } else if let Kind::String(s) = self.peek() {
            let s = s.clone();
            self.next();
            Value::Text(s)
        } else {
            let negative = self.symbol('-');
            if !negative {
                self.symbol('+');
            }
            let Kind::Number(n) = self.next() else {
                return Err(self.error("literal or parameter"));
            };
            let number = if negative { format!("-{n}") } else { n };
            if number.contains(['.', 'e', 'E']) {
                Value::Float(number.parse().map_err(|_| Error::Literal(offset))?)
            } else {
                Value::Integer(number.parse().map_err(|_| Error::Literal(offset))?)
            }
        };
        value.validate().map_err(|_| Error::Literal(offset))?;
        Ok(Scalar::Literal(value))
    }
    fn insert(&mut self) -> Result<Statement> {
        self.require("INTO")?;
        let table = self.identifier()?;
        let columns = if self.symbol('(') {
            let names = self.names()?;
            self.punctuation(')')?;
            Some(names)
        } else {
            None
        };
        self.require("VALUES")?;
        let mut rows = Vec::new();
        loop {
            if rows.len() >= MAX_INSERT_ROWS {
                return Err(Error::Limit("insert rows"));
            }
            self.punctuation('(')?;
            let mut row = Vec::new();
            loop {
                if row.len() >= MAX_COLUMNS {
                    return Err(Error::Limit("row columns"));
                }
                row.push(self.scalar()?);
                if !self.symbol(',') {
                    break;
                }
            }
            self.punctuation(')')?;
            rows.push(row);
            if !self.symbol(',') {
                break;
            }
        }
        Ok(Statement::Insert {
            table,
            columns,
            rows,
        })
    }
    fn column(&mut self) -> Result<ColumnRef> {
        let first = self.identifier()?;
        if self.symbol('.') {
            Ok(ColumnRef {
                table: Some(first),
                name: self.identifier()?,
            })
        } else {
            Ok(ColumnRef {
                table: None,
                name: first,
            })
        }
    }
    fn table(&mut self) -> Result<TableRef> {
        let name = self.identifier()?;
        let alias = if self.word("AS") {
            Some(self.identifier()?)
        } else {
            None
        };
        Ok(TableRef { name, alias })
    }
    fn select(&mut self) -> Result<Select> {
        let columns = if self.symbol('*') {
            None
        } else {
            let mut columns = Vec::new();
            loop {
                if columns.len() >= MAX_COLUMNS {
                    return Err(Error::Limit("projection columns"));
                }
                let column = self.column()?;
                let alias = if self.word("AS") {
                    Some(self.identifier()?)
                } else {
                    None
                };
                columns.push(Projection { column, alias });
                if !self.symbol(',') {
                    break;
                }
            }
            Some(columns)
        };
        self.require("FROM")?;
        let from = self.table()?;
        let inner = self.word("INNER");
        let join = if inner || self.word("JOIN") {
            if inner {
                self.require("JOIN")?;
            }
            let table = self.table()?;
            self.require("ON")?;
            Some((table, self.expression(0)?))
        } else {
            None
        };
        let filter = self.filter()?;
        let mut order = Vec::new();
        if self.word("ORDER") {
            self.require("BY")?;
            loop {
                if order.len() >= MAX_COLUMNS {
                    return Err(Error::Limit("order columns"));
                }
                let column = self.column()?;
                let descending = self.word("DESC");
                if !descending {
                    self.word("ASC");
                }
                let nulls_first = if self.word("NULLS") {
                    if self.word("FIRST") {
                        true
                    } else {
                        self.require("LAST")?;
                        false
                    }
                } else {
                    false
                };
                order.push(Order {
                    column,
                    descending,
                    nulls_first,
                });
                if !self.symbol(',') {
                    break;
                }
            }
        }
        let limit = if self.word("LIMIT") {
            let limit = self.scalar()?;
            if !(matches!(limit, Scalar::Parameter(_))
                || matches!(limit, Scalar::Literal(Value::Integer(n)) if (0..=MAX_RESULT_ROWS as i64).contains(&n)))
            {
                return Err(self.error("bounded nonnegative integer LIMIT"));
            }
            Some(limit)
        } else {
            None
        };
        Ok(Select {
            columns,
            from,
            join,
            filter,
            order,
            limit,
        })
    }
    fn filter(&mut self) -> Result<Option<Expr>> {
        if self.word("WHERE") {
            Ok(Some(self.expression(0)?))
        } else {
            Ok(None)
        }
    }
    fn update(&mut self) -> Result<Statement> {
        let table = self.identifier()?;
        self.require("SET")?;
        let mut assignments = Vec::new();
        loop {
            if assignments.len() >= MAX_COLUMNS {
                return Err(Error::Limit("assignments"));
            }
            let column = self.identifier()?;
            if self.next() != Kind::Operator("=") {
                return Err(self.error("assignment equals"));
            }
            assignments.push((column, self.scalar()?));
            if !self.symbol(',') {
                break;
            }
        }
        Ok(Statement::Update {
            table,
            assignments,
            filter: self.filter()?,
        })
    }
    fn expression(&mut self, depth: usize) -> Result<Expr> {
        let mut expr = self.and(depth)?;
        let mut chain = 0;
        while self.word("OR") {
            chain += 1;
            if chain + depth >= MAX_EXPRESSION_DEPTH {
                return Err(Error::Limit("expression depth"));
            }
            expr = checked(
                Expr::Or(Box::new(expr), Box::new(self.and(depth + chain)?)),
                depth,
            )?;
        }
        Ok(expr)
    }
    fn and(&mut self, depth: usize) -> Result<Expr> {
        let mut expr = self.predicate(depth)?;
        let mut chain = 0;
        while self.word("AND") {
            chain += 1;
            if chain + depth >= MAX_EXPRESSION_DEPTH {
                return Err(Error::Limit("expression depth"));
            }
            expr = checked(
                Expr::And(Box::new(expr), Box::new(self.predicate(depth + chain)?)),
                depth,
            )?;
        }
        Ok(expr)
    }
    fn predicate(&mut self, depth: usize) -> Result<Expr> {
        if depth >= MAX_EXPRESSION_DEPTH {
            return Err(Error::Limit("expression depth"));
        }
        if self.word("NOT") {
            return Ok(Expr::Not(Box::new(self.predicate(depth + 1)?)));
        }
        if self.symbol('(') {
            let expr = self.expression(depth + 1)?;
            self.punctuation(')')?;
            return Ok(expr);
        }
        let left = self.operand()?;
        if self.word("IS") {
            let negated = self.word("NOT");
            self.require("NULL")?;
            return Ok(Expr::IsNull(left, negated));
        }
        let comparison = match self.peek() {
            Kind::Operator("=") => Compare::Eq,
            Kind::Operator("!=") | Kind::Operator("<>") => Compare::Ne,
            Kind::Operator("<") => Compare::Lt,
            Kind::Operator("<=") => Compare::Le,
            Kind::Operator(">") => Compare::Gt,
            Kind::Operator(">=") => Compare::Ge,
            _ => return Ok(Expr::Truth(left)),
        };
        self.next();
        Ok(Expr::Compare(left, comparison, self.operand()?))
    }
    fn operand(&mut self) -> Result<Operand> {
        if matches!(self.peek(),Kind::Word(s) if !["NULL","TRUE","FALSE","X"].iter().any(|w|s.eq_ignore_ascii_case(w)))
            || matches!(self.peek(), Kind::Identifier(_))
        {
            Ok(Operand::Column(self.column()?))
        } else {
            Ok(Operand::Scalar(self.scalar()?))
        }
    }
}

fn checked(expr: Expr, depth: usize) -> Result<Expr> {
    fn visit(expr: &Expr, depth: usize) -> Result<()> {
        if depth >= MAX_EXPRESSION_DEPTH {
            return Err(Error::Limit("expression depth"));
        }
        match expr {
            Expr::And(left, right) | Expr::Or(left, right) => {
                visit(left, depth + 1)?;
                visit(right, depth + 1)
            }
            Expr::Not(inner) => visit(inner, depth + 1),
            _ => Ok(()),
        }
    }
    visit(&expr, depth)?;
    Ok(expr)
}

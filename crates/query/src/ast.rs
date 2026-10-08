use emilybase_catalog::{Schema, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Literal(Value),
    /// One-based position in the caller's separate binding array.
    Parameter(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRef {
    pub table: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    Column(ColumnRef),
    Scalar(Scalar),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Truth(Operand),
    Compare(Operand, Compare, Operand),
    IsNull(Operand, bool),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    pub column: ColumnRef,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub column: ColumnRef,
    pub descending: bool,
    pub nulls_first: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Select {
    /// None means the complete joined row, in table/column declaration order.
    pub columns: Option<Vec<Projection>>,
    pub from: TableRef,
    pub join: Option<(TableRef, Expr)>,
    pub filter: Option<Expr>,
    pub order: Vec<Order>,
    pub limit: Option<Scalar>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Create(Schema),
    Drop(String),
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        rows: Vec<Vec<Scalar>>,
    },
    InsertSelect {
        table: String,
        columns: Option<Vec<String>>,
        select: Box<Select>,
    },
    Select(Box<Select>),
    Update {
        table: String,
        assignments: Vec<(String, Scalar)>,
        filter: Option<Expr>,
    },
    Delete {
        table: String,
        filter: Option<Expr>,
    },
    Begin,
    Commit,
    Rollback,
}

use crate::ast::Compare;
use crate::predicate::{BoundOperand, Predicate};
use emilybase_catalog::Value;

pub(crate) enum PrimaryRange {
    Integer(IntegerRange),
    Text(TextRange),
}
pub(crate) struct TextRange {
    lower: Option<String>,
    upper: Option<String>,
    empty: bool,
}
impl PrimaryRange {
    pub fn bounds(
        &self,
    ) -> (
        Option<emilybase_catalog::Key>,
        Option<emilybase_catalog::Key>,
    ) {
        use emilybase_catalog::Key;
        match self {
            Self::Integer(range) => (range.lower.map(Key::Integer), range.upper.map(Key::Integer)),
            Self::Text(range) => (
                range.lower.clone().map(Key::Text),
                range.upper.clone().map(Key::Text),
            ),
        }
    }
    pub fn empty(&self) -> bool {
        match self {
            Self::Integer(range) => range.empty,
            Self::Text(range) => range.empty,
        }
    }
    pub fn scan(
        &self,
        snapshot: &emilybase_database::Snapshot,
        table: &str,
        limit: usize,
    ) -> emilybase_database::Result<Vec<emilybase_catalog::Row>> {
        match self {
            Self::Integer(range) => {
                snapshot.scan_integer_range(table, range.lower, range.upper, limit)
            }
            Self::Text(range) => snapshot.scan_text_range(
                table,
                range.lower.as_deref(),
                range.upper.as_deref(),
                limit,
            ),
        }
    }
    fn intersect(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Integer(a), Self::Integer(b)) => Some(Self::Integer(a.intersect(b))),
            (Self::Text(a), Self::Text(b)) => {
                let lower = match (a.lower, b.lower) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                };
                let upper = match (a.upper, b.upper) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                let empty =
                    a.empty || b.empty || matches!((&lower, &upper), (Some(a), Some(b)) if a >= b);
                Some(Self::Text(TextRange {
                    lower,
                    upper,
                    empty,
                }))
            }
            _ => None,
        }
    }
}
fn text_constraint(op: Compare, value: &str) -> Option<PrimaryRange> {
    if value.len() > emilybase_index::MAX_KEY_BYTES {
        return None;
    }
    let mut range = TextRange {
        lower: None,
        upper: None,
        empty: false,
    };
    match op {
        Compare::Ge => range.lower = Some(value.into()),
        Compare::Lt => range.upper = Some(value.into()),
        Compare::Gt | Compare::Le => {
            if value.len() == emilybase_index::MAX_KEY_BYTES {
                return None;
            }
            // Appending the minimum Unicode scalar is the exact next string in byte order.
            let successor = format!("{value}\0");
            if op == Compare::Gt {
                range.lower = Some(successor);
            } else {
                range.upper = Some(successor);
            }
        }
        _ => return None,
    }
    Some(PrimaryRange::Text(range))
}

#[derive(Clone, Copy)]
pub(crate) struct IntegerRange {
    pub lower: Option<i64>,
    pub upper: Option<i64>,
    pub empty: bool,
}
impl IntegerRange {
    fn intersect(self, other: Self) -> Self {
        let lower = match (self.lower, other.lower) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        let upper = match (self.upper, other.upper) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        Self {
            lower,
            upper,
            empty: self.empty || other.empty || matches!((lower,upper),(Some(a),Some(b)) if a>=b),
        }
    }
}

/// Extract only necessary AND conjuncts. OR/NOT/column comparisons stay filters.
/// Operands have already been fully bound and schema/type checked.
pub(crate) fn primary_range(predicate: &Predicate, primary: usize) -> Option<PrimaryRange> {
    if let Predicate::And(a, b) = predicate {
        return match (primary_range(a, primary), primary_range(b, primary)) {
            (Some(a), Some(b)) => a.intersect(b),
            (a, b) => a.or(b),
        };
    }
    let (column, op, value) = match predicate {
        Predicate::Compare(BoundOperand::Column(i), op, BoundOperand::Value(v)) => (*i, *op, v),
        Predicate::Compare(BoundOperand::Value(v), op, BoundOperand::Column(i)) => {
            let reverse = match op {
                Compare::Lt => Compare::Gt,
                Compare::Le => Compare::Ge,
                Compare::Gt => Compare::Lt,
                Compare::Ge => Compare::Le,
                _ => *op,
            };
            (*i, reverse, v)
        }
        _ => return None,
    };
    if column != primary {
        return None;
    }
    if let Value::Text(value) = value {
        return text_constraint(op, value);
    }
    let Value::Integer(value) = value else {
        return None;
    };
    let value = *value;
    let mut range = IntegerRange {
        lower: None,
        upper: None,
        empty: false,
    };
    match op {
        Compare::Ge => range.lower = Some(value),
        Compare::Gt => {
            range.lower = value.checked_add(1);
            range.empty = range.lower.is_none();
        }
        Compare::Lt => range.upper = Some(value),
        Compare::Le => range.upper = value.checked_add(1),
        _ => return None,
    }
    Some(PrimaryRange::Integer(range))
}

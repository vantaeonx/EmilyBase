use crate::ast::Compare;
use crate::predicate::{BoundOperand, Predicate};
use emilybase_catalog::Value;

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
pub(crate) fn primary_range(predicate: &Predicate, primary: usize) -> Option<IntegerRange> {
    if let Predicate::And(a, b) = predicate {
        return match (primary_range(a, primary), primary_range(b, primary)) {
            (Some(a), Some(b)) => Some(a.intersect(b)),
            (a, b) => a.or(b),
        };
    }
    let (column, op, value) = match predicate {
        Predicate::Compare(BoundOperand::Column(i), op, BoundOperand::Value(Value::Integer(v))) => {
            (*i, *op, *v)
        }
        Predicate::Compare(BoundOperand::Value(Value::Integer(v)), op, BoundOperand::Column(i)) => {
            let reverse = match op {
                Compare::Lt => Compare::Gt,
                Compare::Le => Compare::Ge,
                Compare::Gt => Compare::Lt,
                Compare::Ge => Compare::Le,
                _ => *op,
            };
            (*i, reverse, *v)
        }
        _ => return None,
    };
    if column != primary {
        return None;
    }
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
    Some(range)
}

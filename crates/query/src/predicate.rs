use crate::ast::{Compare, Expr, Operand, Scalar};
use crate::execute::{Budget, ExecutionError, RunResult};
use crate::plan::Layout;
use crate::row_view::RowView;
use emilybase_catalog::{DataType, Row, Value};
use std::cmp::Ordering;

#[derive(Clone)]
pub(crate) enum BoundOperand {
    Column(usize),
    Value(Value),
}
pub(crate) enum Predicate {
    Truth(BoundOperand),
    Compare(BoundOperand, Compare, BoundOperand),
    IsNull(BoundOperand, bool),
    Not(Box<Self>),
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
}

pub(crate) fn bind(value: &Scalar, parameters: &[Value]) -> RunResult<Value> {
    match value {
        Scalar::Literal(value) => Ok(value.clone()),
        Scalar::Parameter(n) => parameters
            .get(n.saturating_sub(1))
            .cloned()
            .ok_or(ExecutionError::Binding(*n)),
    }
}

impl Predicate {
    pub(crate) fn compile(expr: &Expr, layout: &Layout, parameters: &[Value]) -> RunResult<Self> {
        fn operand(
            value: &Operand,
            layout: &Layout,
            parameters: &[Value],
        ) -> RunResult<(BoundOperand, Option<DataType>)> {
            match value {
                Operand::Column(column) => {
                    let index = layout.column(column)?;
                    Ok((
                        BoundOperand::Column(index),
                        Some(layout.columns[index].data_type),
                    ))
                }
                Operand::Scalar(value) => {
                    let value = bind(value, parameters)?;
                    let kind = value.data_type();
                    Ok((BoundOperand::Value(value), kind))
                }
            }
        }
        Ok(match expr {
            Expr::Truth(value) => {
                let (value, kind) = operand(value, layout, parameters)?;
                if kind.is_some_and(|k| k != DataType::Boolean) {
                    return Err(ExecutionError::Type);
                }
                Self::Truth(value)
            }
            Expr::Compare(left, op, right) => {
                let (left, lk) = operand(left, layout, parameters)?;
                let (right, rk) = operand(right, layout, parameters)?;
                if lk.is_some() && rk.is_some() && lk != rk {
                    return Err(ExecutionError::Type);
                }
                Self::Compare(left, *op, right)
            }
            Expr::IsNull(value, negated) => {
                Self::IsNull(operand(value, layout, parameters)?.0, *negated)
            }
            Expr::Not(expr) => Self::Not(Box::new(Self::compile(expr, layout, parameters)?)),
            Expr::And(a, b) => Self::And(
                Box::new(Self::compile(a, layout, parameters)?),
                Box::new(Self::compile(b, layout, parameters)?),
            ),
            Expr::Or(a, b) => Self::Or(
                Box::new(Self::compile(a, layout, parameters)?),
                Box::new(Self::compile(b, layout, parameters)?),
            ),
        })
    }
    pub(crate) fn evaluate(&self, row: &Row, budget: &mut Budget) -> RunResult<Option<bool>> {
        self.evaluate_view(RowView::single(row), budget)
    }
    pub(crate) fn evaluate_view(
        &self,
        row: RowView<'_>,
        budget: &mut Budget,
    ) -> RunResult<Option<bool>> {
        budget.step()?;
        fn value<'a>(operand: &'a BoundOperand, row: RowView<'a>) -> RunResult<&'a Value> {
            match operand {
                BoundOperand::Value(v) => Ok(v),
                BoundOperand::Column(i) => row.get(*i).ok_or(ExecutionError::Plan),
            }
        }
        Ok(match self {
            Self::Truth(v) => match value(v, row)? {
                Value::Null => None,
                Value::Boolean(v) => Some(*v),
                _ => return Err(ExecutionError::Type),
            },
            Self::IsNull(v, negated) => Some(matches!(value(v, row)?, Value::Null) != *negated),
            Self::Compare(a, op, b) => {
                let (a, b) = (value(a, row)?, value(b, row)?);
                if a == &Value::Null || b == &Value::Null {
                    None
                } else {
                    let order = value_order(a, b).ok_or(ExecutionError::Type)?;
                    Some(match op {
                        Compare::Eq => order.is_eq(),
                        Compare::Ne => !order.is_eq(),
                        Compare::Lt => order.is_lt(),
                        Compare::Le => !order.is_gt(),
                        Compare::Gt => order.is_gt(),
                        Compare::Ge => !order.is_lt(),
                    })
                }
            }
            Self::Not(inner) => inner.evaluate_view(row, budget)?.map(|v| !v),
            Self::And(a, b) => match (a.evaluate_view(row, budget)?, b.evaluate_view(row, budget)?)
            {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            },
            Self::Or(a, b) => {
                match (a.evaluate_view(row, budget)?, b.evaluate_view(row, budget)?) {
                    (Some(true), _) | (_, Some(true)) => Some(true),
                    (Some(false), Some(false)) => Some(false),
                    _ => None,
                }
            }
        })
    }
}

pub(crate) fn value_order(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) => Some(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Text(a), Value::Text(b)) => Some(a.cmp(b)),
        (Value::Bytes(a), Value::Bytes(b)) => Some(a.cmp(b)),
        (Value::Boolean(a), Value::Boolean(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

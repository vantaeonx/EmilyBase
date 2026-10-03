use crate::ast::*;
use crate::execute::{ExecutionError, RunResult, validate_parameters};
use crate::predicate::{BoundOperand, Predicate, bind};
use crate::{MAX_RESULT_ROWS, parse};
use emilybase_catalog::{DataType, Key, Value};
use emilybase_database::Snapshot;

pub(crate) struct Field {
    pub qualifier: String,
    pub name: String,
    pub data_type: DataType,
}
pub(crate) struct Layout {
    pub columns: Vec<Field>,
}
impl Layout {
    pub(crate) fn new(snapshot: &Snapshot, tables: &[&TableRef]) -> RunResult<Self> {
        let mut columns = Vec::new();
        let mut qualifiers = std::collections::BTreeSet::new();
        for table in tables {
            let qualifier = table.alias.as_ref().unwrap_or(&table.name);
            if !qualifiers.insert(qualifier) {
                return Err(ExecutionError::Column);
            }
            for column in &snapshot.schema(&table.name)?.columns {
                columns.push(Field {
                    qualifier: qualifier.clone(),
                    name: column.name.clone(),
                    data_type: column.data_type,
                });
            }
        }
        Ok(Self { columns })
    }
    pub(crate) fn column(&self, reference: &ColumnRef) -> RunResult<usize> {
        let mut found = self.columns.iter().enumerate().filter(|(_, c)| {
            c.name == reference.name && reference.table.as_ref().is_none_or(|t| t == &c.qualifier)
        });
        let index = found.next().map(|(i, _)| i).ok_or(ExecutionError::Column)?;
        if found.next().is_some() {
            return Err(ExecutionError::Column);
        }
        Ok(index)
    }
}

pub(crate) struct SortKey {
    pub index: usize,
    pub descending: bool,
    pub nulls_first: bool,
}
pub(crate) struct Plan {
    pub table: String,
    pub key: Option<Key>,
    pub range: Option<crate::range::PrimaryRange>,
    pub join: Option<(String, Predicate)>,
    pub filter: Option<Predicate>,
    pub columns: Vec<(usize, String)>,
    pub order: Vec<SortKey>,
    pub limit: usize,
}

impl Plan {
    pub(crate) fn compile(
        snapshot: &Snapshot,
        select: &Select,
        parameters: &[Value],
    ) -> RunResult<Self> {
        let mut tables = vec![&select.from];
        if let Some((table, _)) = &select.join {
            tables.push(table);
        }
        let layout = Layout::new(snapshot, &tables)?;
        let columns = match &select.columns {
            Some(columns) => columns
                .iter()
                .map(|c| {
                    Ok((
                        layout.column(&c.column)?,
                        c.alias.clone().unwrap_or_else(|| c.column.name.clone()),
                    ))
                })
                .collect::<RunResult<_>>()?,
            None => layout
                .columns
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    (
                        i,
                        if select.join.is_some() {
                            format!("{}.{}", c.qualifier, c.name)
                        } else {
                            c.name.clone()
                        },
                    )
                })
                .collect(),
        };
        let order = select
            .order
            .iter()
            .map(|o| {
                Ok(SortKey {
                    index: layout.column(&o.column)?,
                    descending: o.descending,
                    nulls_first: o.nulls_first,
                })
            })
            .collect::<RunResult<_>>()?;
        let filter = select
            .filter
            .as_ref()
            .map(|e| Predicate::compile(e, &layout, parameters))
            .transpose()?;
        let join = select
            .join
            .as_ref()
            .map(|(t, e)| -> RunResult<_> {
                Ok((t.name.clone(), Predicate::compile(e, &layout, parameters)?))
            })
            .transpose()?;
        let limit = match &select.limit {
            None => MAX_RESULT_ROWS,
            Some(s) => match bind(s, parameters)? {
                Value::Integer(n) if (0..=MAX_RESULT_ROWS as i64).contains(&n) => n as usize,
                _ => return Err(ExecutionError::Limit("result rows")),
            },
        };
        let key = if select.join.is_none() {
            filter.as_ref().and_then(|e| {
                primary_key(
                    e,
                    usize::from(snapshot.schema(&select.from.name).ok()?.primary_key),
                )
            })
        } else {
            None
        };
        let schema = snapshot.schema(&select.from.name)?;
        let range = if key.is_none() && select.join.is_none() {
            filter.as_ref().and_then(|predicate| {
                crate::range::primary_range(predicate, usize::from(schema.primary_key))
            })
        } else {
            None
        };
        Ok(Self {
            table: select.from.name.clone(),
            key,
            range,
            join,
            filter,
            columns,
            order,
            limit,
        })
    }
}

pub(crate) fn primary_key(predicate: &Predicate, primary: usize) -> Option<Key> {
    match predicate {
        Predicate::Compare(BoundOperand::Column(i), Compare::Eq, BoundOperand::Value(v))
        | Predicate::Compare(BoundOperand::Value(v), Compare::Eq, BoundOperand::Column(i))
            if *i == primary =>
        {
            Key::from_value(v).ok()
        }
        Predicate::And(a, b) => primary_key(a, primary).or_else(|| primary_key(b, primary)),
        _ => None,
    }
}

#[derive(Debug, serde::Serialize)]
pub struct PlanDescription {
    pub access: &'static str,
    pub table: String,
    pub joined_table: Option<String>,
    pub sorted: bool,
    pub limit: usize,
}

/// Compile one SELECT without reading rows or mutating the supplied snapshot.
pub fn explain(snapshot: &Snapshot, sql: &str, parameters: &[Value]) -> RunResult<PlanDescription> {
    validate_parameters(parameters)?;
    let statements = parse(sql)?;
    let [Statement::Select(select)] = statements.as_slice() else {
        return Err(ExecutionError::Control);
    };
    let plan = Plan::compile(snapshot, select, parameters)?;
    Ok(PlanDescription {
        access: if plan.join.is_some() {
            "bounded_nested_loop"
        } else if plan.key.is_some() {
            "primary_key"
        } else if plan.range.is_some() {
            "primary_range"
        } else {
            "scan"
        },
        table: plan.table,
        joined_table: plan.join.map(|(t, _)| t),
        sorted: !plan.order.is_empty(),
        limit: plan.limit,
    })
}

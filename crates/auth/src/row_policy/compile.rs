use super::*;
use emilybase_catalog::{DataType, encode_schema};

struct Budget {
    nodes: usize,
    literals: usize,
}
impl Budget {
    fn node(&mut self, depth: usize) -> Result<()> {
        self.nodes = self.nodes.checked_add(1).ok_or(PolicyError::Limit)?;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES {
            return Err(PolicyError::Limit);
        }
        Ok(())
    }
    fn literal(&mut self, value: &Value) -> Result<()> {
        value.validate().map_err(|_| PolicyError::Schema)?;
        let bytes = match value {
            Value::Text(s) => s.len(),
            Value::Bytes(b) => b.len(),
            _ => 8,
        };
        self.literals = self.literals.checked_add(bytes).ok_or(PolicyError::Limit)?;
        if self.literals > MAX_LITERAL_BYTES {
            return Err(PolicyError::Limit);
        }
        Ok(())
    }
}
fn column(schema: &Schema, name: &str) -> Result<usize> {
    schema
        .columns
        .iter()
        .position(|c| c.name == name)
        .ok_or(PolicyError::Schema)
}
fn expression(
    rule: &Rule,
    schema: &Schema,
    budget: &mut Budget,
    depth: usize,
) -> Result<Expression> {
    budget.node(depth)?;
    Ok(match rule {
        Rule::Deny {} => Expression::Deny,
        Rule::Authenticated {} => Expression::Authenticated,
        Rule::Owner { column: name } => {
            let index = column(schema, name)?;
            if schema.columns[index].data_type != DataType::Bytes {
                return Err(PolicyError::Schema);
            }
            Expression::Owner(index)
        }
        Rule::Equal {
            column: name,
            value,
        } => {
            let index = column(schema, name)?;
            if value.data_type() != Some(schema.columns[index].data_type) {
                return Err(PolicyError::Schema);
            }
            budget.literal(value)?;
            Expression::Equal(index, value.clone())
        }
        Rule::IsNull { column: name } => {
            let index = column(schema, name)?;
            if !schema.columns[index].nullable {
                return Err(PolicyError::Schema);
            }
            Expression::IsNull(index)
        }
        Rule::All { terms } | Rule::Any { terms } => {
            if terms.is_empty() || terms.len() > MAX_NODES - budget.nodes {
                return Err(PolicyError::Limit);
            }
            let expressions = terms
                .iter()
                .map(|rule| expression(rule, schema, budget, depth + 1))
                .collect::<Result<Vec<_>>>()?;
            if matches!(rule, Rule::All { .. }) {
                Expression::All(expressions)
            } else {
                Expression::Any(expressions)
            }
        }
    })
}
pub(super) fn bind(context: TableContext<'_>, definition: &Definition) -> Result<BoundPolicy> {
    if !crate::valid_project_id(context.project) || context.id == 0 {
        return Err(PolicyError::Scope);
    }
    if definition.version != 1 {
        return Err(PolicyError::Version);
    }
    encode_schema(context.schema).map_err(|_| PolicyError::Schema)?;
    let mut budget = Budget {
        nodes: 0,
        literals: 0,
    };
    let rules = [
        &definition.select,
        &definition.insert,
        &definition.update_using,
        &definition.update_check,
        &definition.delete,
    ];
    let mut expressions = [
        Expression::Deny,
        Expression::Deny,
        Expression::Deny,
        Expression::Deny,
        Expression::Deny,
    ];
    for (target, rule) in expressions.iter_mut().zip(rules) {
        *target = expression(rule, context.schema, &mut budget, 1)?;
    }
    Ok(BoundPolicy {
        project: context.project.into(),
        table: context.id,
        schema: context.schema.clone(),
        expressions,
    })
}

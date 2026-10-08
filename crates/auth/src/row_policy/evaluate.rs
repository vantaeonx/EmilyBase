use super::*;
use emilybase_catalog::encode_row;
use subtle::ConstantTimeEq;
fn matches(expression: &Expression, row: &[Value], user: &[u8; 16]) -> bool {
    match expression {
        Expression::Deny => false,
        Expression::Authenticated => true,
        Expression::Owner(index) => match row.get(*index) {
            Some(Value::Bytes(owner)) if owner.len() == 16 => {
                bool::from(owner.as_slice().ct_eq(user))
            }
            _ => false,
        },
        Expression::Equal(index, literal) => row.get(*index) == Some(literal),
        Expression::IsNull(index) => row.get(*index) == Some(&Value::Null),
        Expression::All(terms) => terms.iter().all(|term| matches(term, row, user)),
        Expression::Any(terms) => terms.iter().any(|term| matches(term, row, user)),
    }
}
fn row(policy: &BoundPolicy, row: &[Value]) -> Result<()> {
    policy
        .schema
        .validate_row(row)
        .map_err(|_| PolicyError::Row)?;
    encode_row(row).map_err(|_| PolicyError::Row)?;
    Ok(())
}
pub(super) fn authorize(
    policy: &BoundPolicy,
    context: TableContext<'_>,
    principal: &SessionPrincipal<'_>,
    change: Change<'_>,
) -> Result<()> {
    if context.project != policy.project
        || principal.project() != policy.project
        || context.id != policy.table
        || context.schema != &policy.schema
        || principal.account().disabled
        || principal.account().credential_epoch == 0
    {
        return Err(PolicyError::Scope);
    }
    let user = &principal.account().id;
    let permitted = match change {
        Change::Select(values) => {
            row(policy, values)?;
            matches(&policy.expressions[0], values, user)
        }
        Change::Insert(values) => {
            row(policy, values)?;
            matches(&policy.expressions[1], values, user)
        }
        Change::Update { old, new } => {
            row(policy, old)?;
            row(policy, new)?;
            if policy.schema.key(old).map_err(|_| PolicyError::Row)?
                != policy.schema.key(new).map_err(|_| PolicyError::Row)?
            {
                return Err(PolicyError::Row);
            }
            matches(&policy.expressions[2], old, user) && matches(&policy.expressions[3], new, user)
        }
        Change::Delete(values) => {
            row(policy, values)?;
            matches(&policy.expressions[4], values, user)
        }
    };
    if permitted {
        Ok(())
    } else {
        Err(PolicyError::Denied)
    }
}

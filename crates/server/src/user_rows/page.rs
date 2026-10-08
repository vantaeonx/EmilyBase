//! Current-state keyset pages retain only rows allowed by the current SELECT rule.
use super::*;

pub(super) fn run(
    database: &Database,
    proof: &PolicyPrincipal<'_>,
    context: TableContext<'_>,
    after: Option<&Key>,
    limit: usize,
) -> Result<UserTableResult, UserRowsError> {
    let snapshot = database.view().map_err(transaction)?;
    // The original cursor validates key types/physical pointers and caps source
    // rows at MAX_ROWS. No complete owned scan or hidden-key continuation is made.
    let mut selected = Vec::with_capacity(limit);
    let mut last = None;
    let mut more = false;
    for item in snapshot.primary_rows(&context.schema.name, after, None)? {
        let row = item?;
        match proof.authorize(context, Change::Select(row)) {
            Ok(()) => {}
            Err(PolicyError::Denied) => continue,
            Err(error) => return Err(UserRowsError::Policy(error)),
        }
        let key = context.schema.key(row).map_err(|_| UserRowsError::Input)?;
        if after.is_some_and(|bound| key <= *bound) {
            continue;
        }
        // A continuation exists only if another permitted row exists. Merely
        // seeing a hidden row must reveal neither its key nor a next-page flag.
        if selected.len() == limit {
            more = true;
            break;
        }
        selected.push(row);
        last = Some(key);
    }
    Ok(UserTableResult::Page {
        rows: selected.into_iter().cloned().collect(),
        next: if more { last } else { None },
    })
}

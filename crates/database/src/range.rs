use crate::{Error, MAX_ROWS, Result, Snapshot};
use emilybase_catalog::{DataType, Key, Row};

impl Snapshot {
    /// Ordered integer primary-key interval: inclusive lower, exclusive upper.
    /// Validate schema/bounds even for empty results; do not mutate physical pages.
    pub fn scan_integer_range(
        &self,
        name: &str,
        lower: Option<i64>,
        upper: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Row>> {
        let schema = self.schema(name)?;
        if schema.columns[usize::from(schema.primary_key)].data_type != DataType::Integer {
            return Err(Error::IntegerRangeType);
        }
        if limit > MAX_ROWS {
            return Err(Error::Limit("range rows"));
        }
        if limit == 0 || matches!((lower,upper),(Some(a),Some(b)) if a>=b) {
            return Ok(Vec::new());
        }
        let table_id = self.table_id(name)?;
        let table = self.state.table(name)?;
        let tree = self
            .primary_indexes
            .tree(table_id, &table.rows, &self.locations)?;
        let lower = lower.map(Key::Integer);
        let upper = upper.map(Key::Integer);
        let entries = tree
            .range(lower.as_ref(), upper.as_ref(), limit)
            .map_err(|_| Error::PrimaryIndex("range lookup failed"))?;
        let expected = table
            .rows
            .range((
                lower
                    .as_ref()
                    .map_or(std::ops::Bound::Unbounded, std::ops::Bound::Included),
                upper
                    .as_ref()
                    .map_or(std::ops::Bound::Unbounded, std::ops::Bound::Excluded),
            ))
            .take(limit);
        // Compare bounded ordered keys to the authoritative live representation.
        // A missing or extra derived entry must fail instead of silently losing rows.
        if !entries
            .iter()
            .map(|(key, _)| key)
            .eq(expected.map(|(key, _)| key))
        {
            return Err(Error::PrimaryIndex("range key mismatch"));
        }
        entries
            .into_iter()
            .map(|(key, pointer)| {
                let location = self
                    .locations
                    .get(table_id, &key)
                    .ok_or(Error::PrimaryIndex("unknown range key"))?;
                if (pointer.page_id, pointer.slot_id) != (location.page_id, location.slot_id) {
                    return Err(Error::PrimaryIndex("obsolete range pointer"));
                }
                Ok(self.resolve_row_location(name, &key, location)?.clone())
            })
            .collect()
    }
}

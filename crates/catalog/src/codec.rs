use crate::{
    Column, DataType, Error, MAX_COLUMNS, MAX_ENCODED_BYTES, RECORD_VERSION, Result, Row, Schema,
    Value,
};

pub fn encode_schema(schema: &Schema) -> Result<Vec<u8>> {
    schema.validate()?;
    let mut output = Writer::new(b"ESCH");
    output.u16(schema.primary_key)?;
    output.u16(schema.columns.len() as u16)?;
    output.blob(schema.name.as_bytes())?;
    for column in &schema.columns {
        output.blob(column.name.as_bytes())?;
        output.byte(match column.data_type {
            DataType::Boolean => 1,
            DataType::Integer => 2,
            DataType::Float => 3,
            DataType::Text => 4,
            DataType::Bytes => 5,
        })?;
        output.byte(u8::from(column.nullable))?;
    }
    Ok(output.bytes)
}

pub fn decode_schema(bytes: &[u8]) -> Result<Schema> {
    let mut input = Reader::new(bytes, b"ESCH")?;
    let primary_key = input.u16()?;
    let count = usize::from(input.u16()?);
    if count == 0 || count > MAX_COLUMNS {
        return Err(Error::ColumnCount);
    }
    let name = input.text()?;
    let mut columns = Vec::with_capacity(count);
    for _ in 0..count {
        let name = input.text()?;
        let data_type = match input.byte()? {
            1 => DataType::Boolean,
            2 => DataType::Integer,
            3 => DataType::Float,
            4 => DataType::Text,
            5 => DataType::Bytes,
            _ => return Err(Error::Decode("column type")),
        };
        let nullable = input.boolean()?;
        columns.push(Column {
            name,
            data_type,
            nullable,
        });
    }
    input.finish()?;
    let schema = Schema {
        name,
        columns,
        primary_key,
    };
    schema.validate()?;
    Ok(schema)
}

pub fn encode_row(row: &[Value]) -> Result<Vec<u8>> {
    if row.len() > MAX_COLUMNS {
        return Err(Error::RowLength);
    }
    let mut output = Writer::new(b"EROW");
    output.u16(row.len() as u16)?;
    for value in row {
        value.validate()?;
        match value {
            Value::Null => output.byte(0)?,
            Value::Boolean(value) => {
                output.byte(1)?;
                output.byte(u8::from(*value))?;
            }
            Value::Integer(value) => {
                output.byte(2)?;
                output.extend(&value.to_le_bytes())?;
            }
            Value::Float(value) => {
                output.byte(3)?;
                output.extend(&value.to_bits().to_le_bytes())?;
            }
            Value::Text(value) => {
                output.byte(4)?;
                output.blob(value.as_bytes())?;
            }
            Value::Bytes(value) => {
                output.byte(5)?;
                output.blob(value)?;
            }
        }
    }
    Ok(output.bytes)
}

pub fn decode_row(bytes: &[u8]) -> Result<Row> {
    let mut input = Reader::new(bytes, b"EROW")?;
    let count = usize::from(input.u16()?);
    if count > MAX_COLUMNS {
        return Err(Error::RowLength);
    }
    let mut row = Vec::with_capacity(count);
    for _ in 0..count {
        let value = match input.byte()? {
            0 => Value::Null,
            1 => Value::Boolean(input.boolean()?),
            2 => Value::Integer(i64::from_le_bytes(input.number()?)),
            3 => Value::Float(f64::from_bits(u64::from_le_bytes(input.number()?))),
            4 => Value::Text(input.text()?),
            5 => Value::Bytes(input.blob()?.to_vec()),
            _ => return Err(Error::Decode("value tag")),
        };
        value.validate()?;
        row.push(value);
    }
    input.finish()?;
    Ok(row)
}

struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn new(magic: &[u8; 4]) -> Self {
        let mut bytes = magic.to_vec();
        bytes.extend_from_slice(&RECORD_VERSION.to_le_bytes());
        Self { bytes }
    }

    fn byte(&mut self, value: u8) -> Result<()> {
        self.extend(&[value])
    }

    fn u16(&mut self, value: u16) -> Result<()> {
        self.extend(&value.to_le_bytes())
    }

    fn blob(&mut self, value: &[u8]) -> Result<()> {
        let size = u16::try_from(value.len()).map_err(|_| Error::RecordSize)?;
        self.u16(size)?;
        self.extend(value)
    }

    fn extend(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > MAX_ENCODED_BYTES.saturating_sub(self.bytes.len()) {
            return Err(Error::RecordSize);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], magic: &[u8; 4]) -> Result<Self> {
        if bytes.len() > MAX_ENCODED_BYTES {
            return Err(Error::RecordSize);
        }
        let mut input = Self { bytes, cursor: 0 };
        if input.take(4)? != magic {
            return Err(Error::Decode("magic"));
        }
        let version = input.u16()?;
        if version != RECORD_VERSION {
            return Err(Error::Version(version));
        }
        Ok(input)
    }

    fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        let end = self
            .cursor
            .checked_add(size)
            .ok_or(Error::Decode("length overflow"))?;
        let bytes = self
            .bytes
            .get(self.cursor..end)
            .ok_or(Error::Decode("truncated record"))?;
        self.cursor = end;
        Ok(bytes)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn boolean(&mut self) -> Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Decode("boolean")),
        }
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn number(&mut self) -> Result<[u8; 8]> {
        let mut value = [0; 8];
        value.copy_from_slice(self.take(8)?);
        Ok(value)
    }

    fn blob(&mut self) -> Result<&'a [u8]> {
        let size = usize::from(self.u16()?);
        self.take(size)
    }

    fn text(&mut self) -> Result<String> {
        std::str::from_utf8(self.blob()?)
            .map(str::to_owned)
            .map_err(|_| Error::Decode("UTF-8"))
    }

    fn finish(self) -> Result<()> {
        if self.cursor != self.bytes.len() {
            return Err(Error::Decode("trailing bytes"));
        }
        Ok(())
    }
}

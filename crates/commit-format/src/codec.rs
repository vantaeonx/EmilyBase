use crate::{Error, Result, VERSION};

pub(crate) fn read<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    let end = offset.checked_add(N).ok_or(Error::Length)?;
    bytes
        .get(offset..end)
        .ok_or(Error::Length)?
        .try_into()
        .map_err(|_| Error::Length)
}

pub(crate) fn number(bytes: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(read(bytes, offset)?))
}

pub(crate) fn verify(bytes: &[u8], size: usize, magic: &[u8; 8]) -> Result<()> {
    if bytes.len() != size {
        return Err(Error::Length);
    }
    if &read::<8>(bytes, 0)? != magic {
        return Err(Error::Magic);
    }
    let version = u16::from_le_bytes(read(bytes, 8)?);
    if version != VERSION {
        return Err(Error::Version(version));
    }
    let position = size.checked_sub(4).ok_or(Error::Length)?;
    let checksum = u32::from_le_bytes(read(bytes, position)?);
    if crc32fast::hash(&bytes[..position]) != checksum {
        return Err(Error::Checksum);
    }
    Ok(())
}

pub(crate) fn finish<const N: usize>(bytes: &mut [u8; N]) {
    let position = N - 4;
    let checksum = crc32fast::hash(&bytes[..position]);
    bytes[position..].copy_from_slice(&checksum.to_le_bytes());
}

use emilybase_transactions::{RecoveredImage, recover_image};
use sha2::{Digest, Sha256};

use crate::{Error, HEADER_SIZE, MAX_BACKUP_BYTES, Report, Result, header};

pub fn encode(wal: &[u8]) -> Result<Vec<u8>> {
    let recovered = recover_image(wal, None)?;
    let report = report(&recovered)?;
    let digest = Sha256::digest(wal).into();
    let mut bytes = Vec::with_capacity(HEADER_SIZE + wal.len());
    bytes.extend_from_slice(&header::encode(&report, digest));
    bytes.extend_from_slice(wal);
    Ok(bytes)
}

/// An owned, validated original-engine image, without filesystem authority.
/// Keeping this value retains its bounded decoded snapshot; it is not a heap quota.
pub struct VerifiedBackup {
    image: RecoveredImage,
    report: Report,
}
impl std::fmt::Debug for VerifiedBackup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VerifiedBackup(contents redacted)")
    }
}
impl VerifiedBackup {
    pub fn image(&self) -> &RecoveredImage {
        &self.image
    }
    pub fn report(&self) -> &Report {
        &self.report
    }
}

/// Verify once and retain the replayed snapshot for additional semantic checks.
/// No filesystem access, owner creation, credentials or request permissions.
pub fn decode_verified(bytes: &[u8]) -> Result<VerifiedBackup> {
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(Error::Limit);
    }
    let header_bytes = bytes
        .get(..HEADER_SIZE)
        .ok_or(Error::Format("truncated header"))?;
    let metadata = header::decode(header_bytes)?;
    if bytes.len() != HEADER_SIZE + metadata.wal_bytes {
        return Err(Error::Format("payload length or trailing bytes"));
    }
    let wal = &bytes[HEADER_SIZE..];
    let digest: [u8; 32] = Sha256::digest(wal).into();
    if digest != metadata.digest {
        return Err(Error::Checksum);
    }
    let image = recover_image(wal, Some(metadata.id))?;
    let report = report(&image)?;
    if report.last_transaction != metadata.transaction
        || report.wal_bytes != metadata.wal_bytes
        || report.wal_version != metadata.wal_version
    {
        return Err(Error::Format(
            "header does not match recovered commit boundary",
        ));
    }
    Ok(VerifiedBackup { image, report })
}

/// Verify length, header CRC, payload SHA-256, identity and complete table replay.
pub fn inspect_bytes(bytes: &[u8]) -> Result<Report> {
    Ok(decode_verified(bytes)?.report)
}

pub(crate) fn report(recovered: &RecoveredImage) -> Result<Report> {
    if recovered.discarded_bytes != 0 {
        return Err(Error::Format("backup contains an uncommitted tail"));
    }
    Ok(Report {
        database_id: recovered.database_id,
        last_transaction: recovered.last_transaction,
        wal_version: recovered.wal_version,
        wal_bytes: recovered.committed_bytes,
        tables: recovered.snapshot.schemas().len(),
        rows: recovered.snapshot.row_count(),
        pages: recovered.snapshot.page_count(),
    })
}

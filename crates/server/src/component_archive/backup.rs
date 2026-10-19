//! Native coordinated backup under the caller's actual source owners.
use super::publication::{RetainedArchive, Target, check_target, checkpoint, publish_retained};
use crate::{AccountFileArchiveReport, AccountRoot, Error, Result, component_snapshot};
use emilybase_files::NativeFileRoot;
use std::path::Path;

/// Back up every binding of an explicitly opened v2 root using its internally
/// retained native owners. The unchanged full manifest is archive authority.
/// The destination must lie outside the original common root; no subset option.
pub fn backup_account_file_root_v2(
    root: &mut AccountRoot,
    path: impl AsRef<Path>,
) -> Result<AccountFileArchiveReport> {
    // Refuse a v1 root before opening a destination or beginning publication.
    root.file_manifest()?
        .ok_or(Error::BundleRoot("explicit v2 backup requires v2 root"))?;
    let target = Target::new(path.as_ref())?;
    let outside = [root.outside_directory(&target.parent)?];
    check_target(&target, &outside)?;
    let mut selected = None;
    let mut uncertain = false;
    let result = root.capture_v2_with(|snapshot| {
        let publication = publish_retained(snapshot, &target, &outside);
        uncertain = matches!(&publication, Err(Error::ComponentPublicationUnknown(_)));
        selected = Some(publication?);
        Ok(())
    });
    confirm(result, selected, uncertain, &target, &outside)
}

/// Publish a joined archive while original account/file owners and every capture
/// payload reader span publication and their respective final source proofs.
/// Output must be outside all original source roots. Existing names are retained.
/// A late source/destination failure withholds the receipt and preserves output.
/// This grants no current user authority, encryption or live v2 adoption.
pub fn backup_account_file_components(
    accounts: &mut AccountRoot,
    files: &mut [NativeFileRoot],
    path: impl AsRef<Path>,
) -> Result<AccountFileArchiveReport> {
    if files.len() > crate::MAX_COMPONENT_FILE_STORES {
        return Err(Error::Limit);
    }
    let target = Target::new(path.as_ref())?;
    let mut outside = Vec::with_capacity(files.len() + 1);
    outside.push(accounts.outside_directory(&target.parent)?);
    for file in files.iter_mut() {
        outside.push(file.outside_directory(&target.parent)?);
    }
    check_target(&target, &outside)?;
    let mut selected = None;
    let mut uncertain = false;
    let result = component_snapshot::capture_with(accounts, files, &mut |snapshot| {
        let publication = publish_retained(snapshot, &target, &outside);
        uncertain = matches!(&publication, Err(Error::ComponentPublicationUnknown(_)));
        selected = Some(publication?);
        Ok(())
    });
    confirm(result, selected, uncertain, &target, &outside)
}
fn confirm(
    result: Result<crate::AccountFileSnapshot>,
    selected: Option<RetainedArchive>,
    uncertain: bool,
    target: &Target,
    outside: &[emilybase_storage::OutsideDirectoryGuard],
) -> Result<AccountFileArchiveReport> {
    if let Err(error) = result {
        return if selected.is_some() || uncertain {
            Err(Error::ComponentPublicationUnknown(Box::new(error)))
        } else {
            Err(error)
        };
    }
    let archive = selected.ok_or(Error::ComponentArchive("missing backup publication"))?;
    // Keep the actual selected archive through the last source checks above.
    // Every payload reader spans its own native final proof; none is invented
    // after callback unwind or promised to remain until this function returns.
    checkpoint("joined_backup_confirming");
    archive
        .check(target, outside)
        .map_err(|error| Error::ComponentPublicationUnknown(Box::new(error)))?;
    Ok(archive.report)
}

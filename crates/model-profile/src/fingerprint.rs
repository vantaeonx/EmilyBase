use crate::support;
use emilybase_index::{BPlusTree, IndexSnapshot, RecordPointer};
use emilybase_model_profile::{Comparison, Components, Config, Error, PhaseKind, Report};
use sha2::{Digest, Sha256};

pub fn measure(config: Config) -> Result<Report, Error> {
    config.validate()?;
    let mut phases = Vec::with_capacity(8);
    let mut snapshots = Vec::with_capacity(config.projects as usize);
    let mut components_per_project = Vec::with_capacity(config.projects as usize);
    for _ in 0..config.projects {
        let entries: Vec<_> = (0..config.rows)
            .map(|number| {
                (
                    support::key(config.kind, number),
                    RecordPointer {
                        page_id: u64::from(number) + 1,
                        slot_id: 0,
                    },
                )
            })
            .collect();
        let tree = BPlusTree::from_sorted_stable(&entries)?;
        let index_bytes = (tree.page_count() as u64 + 1) * 4096;
        components_per_project.push(Components {
            history_pages: 0,
            index_pages: tree.page_count() as u64,
            root_bytes: 0,
            index_bytes,
            total_bytes: index_bytes,
        });
        snapshots.push(IndexSnapshot { revision: 1, tree });
    }
    let mut digests = Vec::with_capacity(config.projects as usize);
    if support::heap().allocated_blocks == 0 {
        return Err(Error::Instrumentation);
    }
    support::sample(&mut phases, PhaseKind::Built);
    let before = support::heap();
    for snapshot in &snapshots {
        let digest: [u8; 32] = Sha256::digest(snapshot.encode()?).into();
        digests.push(digest);
    }
    let full = support::heap();
    support::sample(&mut phases, PhaseKind::FullEncoding);
    for (snapshot, expected) in snapshots.iter().zip(&digests) {
        if std::hint::black_box(snapshot.fingerprint()?) != *expected {
            return Err(Error::Verification);
        }
    }
    let streamed = support::heap();
    support::sample(&mut phases, PhaseKind::Streamed);
    let comparison = Some(Comparison::from_samples(before, full, streamed)?);
    drop(snapshots);
    drop(digests);
    support::sample(&mut phases, PhaseKind::Released);
    Ok(Report {
        version: 1,
        config,
        phases,
        components_per_project,
        comparison,
        images_per_project: None,
    })
}

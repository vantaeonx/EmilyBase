use crate::{Config, Error, Kind, Mode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Heap {
    pub current_bytes: u64,
    pub current_blocks: u64,
    pub peak_bytes: u64,
    pub peak_blocks: u64,
    pub allocated_bytes: u64,
    pub allocated_blocks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PhaseKind {
    Built,
    Staged,
    IndexesStaged,
    Prepared,
    Published,
    OldViewsReleased,
    FullEncoding,
    Streamed,
    PlansBuilt,
    Replayed,
    ReplayReleased,
    PlansReleased,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Phase {
    pub phase: PhaseKind,
    pub heap: Heap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub full_encoding_bytes: u64,
    pub full_encoding_blocks: u64,
    pub streaming_bytes: u64,
    pub streaming_blocks: u64,
}

impl Comparison {
    pub fn from_samples(before: Heap, full: Heap, streamed: Heap) -> Result<Self, Error> {
        Ok(Self {
            full_encoding_bytes: full
                .allocated_bytes
                .checked_sub(before.allocated_bytes)
                .ok_or(Error::Counter)?,
            full_encoding_blocks: full
                .allocated_blocks
                .checked_sub(before.allocated_blocks)
                .ok_or(Error::Counter)?,
            streaming_bytes: streamed
                .allocated_bytes
                .checked_sub(full.allocated_bytes)
                .ok_or(Error::Counter)?,
            streaming_blocks: streamed
                .allocated_blocks
                .checked_sub(full.allocated_blocks)
                .ok_or(Error::Counter)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Components {
    pub history_pages: u64,
    pub index_pages: u64,
    pub root_bytes: u64,
    pub index_bytes: u64,
    pub total_bytes: u64,
}

/// Counts for this diagnostic's one-table, one-replacement workload only.
/// Image bytes exclude envelopes and every retained/replayed memory object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageComponents {
    pub history_pages: u64,
    pub primary_pages: u64,
    pub retired_pages: u64,
    pub changed_roots: u64,
    pub retired_tables: u64,
    pub image_body_bytes: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub version: u16,
    pub config: Config,
    pub phases: Vec<Phase>,
    pub components_per_project: Vec<Components>,
    pub comparison: Option<Comparison>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images_per_project: Option<Vec<ImageComponents>>,
}

impl Report {
    /// Structural/counter consistency only. Unsigned reports do not authenticate
    /// an external measurement or establish a server memory admission policy.
    pub fn validate(&self) -> Result<(), Error> {
        self.config.validate()?;
        let replay = matches!(self.config.mode, Mode::Replay | Mode::IndexReplay);
        let version = if self.config.parallel {
            3
        } else if replay {
            2
        } else {
            1
        };
        if self.version != version
            || self.components_per_project.len() != self.config.projects as usize
        {
            return Err(Error::Report("version or project count"));
        }
        let expected: &[PhaseKind] = match self.config.mode {
            Mode::State | Mode::IndexOnly => &[
                PhaseKind::Built,
                PhaseKind::Staged,
                PhaseKind::IndexesStaged,
                PhaseKind::Prepared,
                PhaseKind::Published,
                PhaseKind::OldViewsReleased,
                PhaseKind::Released,
            ],
            Mode::Fingerprint => &[
                PhaseKind::Built,
                PhaseKind::FullEncoding,
                PhaseKind::Streamed,
                PhaseKind::Released,
            ],
            Mode::Replay | Mode::IndexReplay => &[
                PhaseKind::Built,
                PhaseKind::Staged,
                PhaseKind::IndexesStaged,
                PhaseKind::Prepared,
                PhaseKind::PlansBuilt,
                PhaseKind::Replayed,
                PhaseKind::ReplayReleased,
                PhaseKind::PlansReleased,
                PhaseKind::Published,
                PhaseKind::OldViewsReleased,
                PhaseKind::Released,
            ],
        };
        if self
            .phases
            .iter()
            .map(|phase| phase.phase)
            .ne(expected.iter().copied())
        {
            return Err(Error::Report("phase sequence"));
        }
        let mut previous = None::<Heap>;
        for phase in &self.phases {
            let heap = phase.heap;
            if heap.current_bytes > heap.peak_bytes
                || heap.peak_bytes > heap.allocated_bytes
                || heap.current_blocks > heap.allocated_blocks
                || heap.peak_blocks > heap.allocated_blocks
            {
                return Err(Error::Report("heap counters"));
            }
            if let Some(before) = previous
                && (heap.peak_bytes < before.peak_bytes
                    || heap.allocated_bytes < before.allocated_bytes
                    || heap.allocated_blocks < before.allocated_blocks)
            {
                return Err(Error::Report("regressed counters"));
            }
            previous = Some(heap);
        }
        for value in &self.components_per_project {
            let eligible = if self.config.kind == Kind::LongText {
                0
            } else {
                u64::from(self.config.rows)
            };
            let minimum = eligible.div_ceil(14).max(1);
            let maximum = (8 * eligible / 49 + 1).min(1024);
            if !(1..=1024).contains(&value.index_pages)
                || !(minimum..=maximum).contains(&value.index_pages)
                || value.index_bytes != (value.index_pages + 1) * 4096
            {
                return Err(Error::Report("index component"));
            }
            let total = match self.config.mode {
                Mode::State | Mode::IndexOnly | Mode::Replay | Mode::IndexReplay => {
                    if !(1..=65536).contains(&value.history_pages) || value.root_bytes != 192 {
                        return Err(Error::Report("state component"));
                    }
                    value.history_pages * 4096 + value.index_bytes + value.root_bytes
                }
                Mode::Fingerprint => {
                    if value.history_pages != 0 || value.root_bytes != 0 {
                        return Err(Error::Report("standalone component"));
                    }
                    value.index_bytes
                }
            };
            if total != value.total_bytes {
                return Err(Error::Report("component total"));
            }
        }
        match (self.config.mode, self.comparison) {
            (Mode::State | Mode::IndexOnly | Mode::Replay | Mode::IndexReplay, None) => {}
            (Mode::Fingerprint, Some(comparison)) => {
                let actual = Comparison::from_samples(
                    self.phases[0].heap,
                    self.phases[1].heap,
                    self.phases[2].heap,
                )?;
                if comparison != actual {
                    return Err(Error::Report("allocation comparison"));
                }
            }
            _ => return Err(Error::Report("comparison mode")),
        }
        match (replay, &self.images_per_project) {
            (false, None) => {}
            (true, Some(images)) if images.len() == self.config.projects as usize => {
                for (image, components) in images.iter().zip(&self.components_per_project) {
                    let expected_history = u64::from(self.config.mode == Mode::Replay);
                    if image.history_pages != expected_history
                        || image.changed_roots != 1
                        || image.retired_tables != 0
                        || image.primary_pages > components.index_pages
                        || image.retired_pages > 1024
                        || ((self.config.kind == Kind::LongText
                            || self.config.mode == Mode::IndexReplay)
                            && (image.primary_pages != 0 || image.retired_pages != 0))
                        || (self.config.kind != Kind::LongText
                            && self.config.mode == Mode::Replay
                            && image.primary_pages == 0)
                    {
                        return Err(Error::Report("image component shape"));
                    }
                    let body = image
                        .history_pages
                        .checked_add(image.primary_pages)
                        .and_then(|count| count.checked_mul(4096))
                        .ok_or(Error::Counter)?;
                    if body != image.image_body_bytes {
                        return Err(Error::Report("image body bytes"));
                    }
                }
            }
            _ => return Err(Error::Report("image component mode/count")),
        }
        Ok(())
    }
}

//! Synthetic, opt-in allocation diagnostics. No runtime server dependency.
mod report;
pub use report::{Comparison, Components, Heap, Phase, PhaseKind, Report};
pub const MAX_REPORT_BYTES: usize = 8192;

/// Bounded diagnostic JSON admission. Errors do not echo the supplied document.
pub fn decode_report(bytes: &[u8]) -> Result<Report, Error> {
    if bytes.is_empty() || bytes.len() > MAX_REPORT_BYTES {
        return Err(Error::Report("input length"));
    }
    let report: Report = serde_json::from_slice(bytes).map_err(|_| Error::Report("JSON shape"))?;
    report.validate()?;
    Ok(report)
}

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    State,
    Fingerprint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Integer,
    ShortText,
    LongText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Parser, Serialize, Deserialize)]
#[command(
    name = "emilybase-model-profile",
    about = "Synthetic requested-allocation diagnostics"
)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[arg(long, value_enum, default_value = "state")]
    pub mode: Mode,
    #[arg(long = "case", value_enum, default_value = "integer")]
    pub kind: Kind,
    #[arg(long, default_value_t = 256)]
    pub rows: u16,
    #[arg(long, default_value_t = 1)]
    pub projects: u8,
    #[arg(long, default_value_t = 16)]
    pub value_bytes: u16,
    #[arg(long)]
    pub retain_old: bool,
}

impl Config {
    pub fn validate(self) -> Result<Self, Error> {
        if self.rows == 0
            || self.rows > 10000
            || self.projects == 0
            || self.projects > 4
            || self.value_bytes > 768
        {
            return Err(Error::Config);
        }
        if self.mode == Mode::Fingerprint && (self.kind == Kind::LongText || self.retain_old) {
            return Err(Error::Unsupported);
        }
        Ok(self)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("profiling input must use rows 1..10000, projects 1..4 and value bytes 0..768")]
    Config,
    #[error("fingerprint mode accepts integer/short-text cases and no retained model view")]
    Unsupported,
    #[error("allocation counters regressed")]
    Counter,
    #[error("allocation instrumentation is not active")]
    Instrumentation,
    #[error("invalid profiling report: {0}")]
    Report(&'static str),
    #[error("synthetic profile state failed verification")]
    Verification,
    #[error(transparent)]
    Model(#[from] emilybase_commit_model::Error),
    #[error(transparent)]
    Metadata(#[from] emilybase_commit_format::Error),
    #[error(transparent)]
    Database(#[from] emilybase_database::Error),
    #[error(transparent)]
    Index(#[from] emilybase_index::Error),
}

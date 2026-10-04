use std::fs::File;
use std::io::{self, Read, Seek, Write};

/// Private synchronous boundary for deterministic I/O failure tests.
/// Production construction always uses a locked standard filesystem file.
pub(crate) trait JournalIo: Read + Write + Seek + Send {
    fn metadata(&self) -> io::Result<std::fs::Metadata>;
    fn length(&self) -> io::Result<u64>;
    fn truncate(&self, length: u64) -> io::Result<()>;
    fn sync(&self) -> io::Result<()>;
}

impl JournalIo for File {
    fn metadata(&self) -> io::Result<std::fs::Metadata> {
        File::metadata(self)
    }
    fn length(&self) -> io::Result<u64> {
        Ok(self.metadata()?.len())
    }
    fn truncate(&self, length: u64) -> io::Result<()> {
        self.set_len(length)
    }
    fn sync(&self) -> io::Result<()> {
        self.sync_all()
    }
}

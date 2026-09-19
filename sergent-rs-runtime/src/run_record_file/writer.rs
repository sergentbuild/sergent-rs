//! Serialized writer authority, boundary acknowledgement, and close lifecycle.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use super::RunRecordFileError;

/// The private external-system seam used by file production and deterministic tests.
pub(crate) trait RecordFileWriter: Send {
    /// Attempt one possibly partial byte write.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize>;
    /// Flush buffered bytes to the stream boundary.
    fn flush(&mut self) -> io::Result<()>;
    /// Synchronize stream contents to storage.
    fn sync(&mut self) -> io::Result<()>;
    /// Close the underlying stream once.
    fn close(&mut self) -> io::Result<()>;
}

/// The production writer owning one optionally open file.
pub(crate) struct FileWriter {
    file: Option<File>,
}

impl FileWriter {
    /// Exclusively create one write-only file with owner-only Unix mode.
    pub(crate) fn create(path: &Path) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path).map(|file| Self { file: Some(file) })
    }

    /// Borrow the file only while its production lifecycle remains open.
    fn file(&mut self) -> io::Result<&mut File> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("Run Record file is closed"))
    }
}

impl RecordFileWriter for FileWriter {
    /// Delegate one partial write to the owned file.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        std::io::Write::write(self.file()?, bytes)
    }

    /// Flush the owned file buffer.
    fn flush(&mut self) -> io::Result<()> {
        std::io::Write::flush(self.file()?)
    }

    /// Synchronize the owned file to storage.
    fn sync(&mut self) -> io::Result<()> {
        self.file()?.sync_all()
    }

    /// Drop the sole file owner once.
    fn close(&mut self) -> io::Result<()> {
        self.file.take();
        Ok(())
    }
}

/// A line's post-flush acknowledgement responsibility.
pub(crate) enum BoundaryAck {
    None,
    Start(String),
    End,
}

/// One writer guard's lifecycle and acknowledged Run starts.
pub(crate) struct LogState {
    lifecycle: Lifecycle,
    acknowledged_starts: HashSet<String>,
}

/// The mutually exclusive healthy, failed, and closed stream states.
enum Lifecycle {
    Active(Box<dyn RecordFileWriter>),
    Failed { first: RunRecordFileError },
    Closed,
}

impl LogState {
    /// Open one unacknowledged lifecycle over the supplied writer.
    pub(crate) fn active(writer: Box<dyn RecordFileWriter>) -> Self {
        Self {
            lifecycle: Lifecycle::Active(writer),
            acknowledged_starts: HashSet::new(),
        }
    }

    /// Persist and acknowledge one line, or retain the first ambiguous failure.
    pub(crate) fn record(
        &mut self,
        bytes: &[u8],
        boundary: BoundaryAck,
    ) -> Result<(), RunRecordFileError> {
        if self.is_acknowledged(&boundary) {
            return Ok(());
        }
        let result = match &mut self.lifecycle {
            Lifecycle::Active(writer) => persist(writer.as_mut(), bytes, sync_required(&boundary)),
            Lifecycle::Failed { first, .. } => return Err(RunRecordFileError::unavailable(first)),
            Lifecycle::Closed => return Err(RunRecordFileError::closed()),
        };
        if let Err(error) = result {
            self.lifecycle = Lifecycle::Failed {
                first: error.clone(),
            };
            return Err(error);
        }
        self.acknowledge(boundary);
        Ok(())
    }

    /// Synchronize and close active state once without replaying older failure.
    pub(crate) fn close(&mut self) -> Result<(), RunRecordFileError> {
        match &mut self.lifecycle {
            Lifecycle::Closed => Ok(()),
            Lifecycle::Failed { .. } => Ok(()),
            Lifecycle::Active(writer) => {
                let sync = writer.sync();
                let close = writer.close();
                let failure = sync
                    .err()
                    .map(|error| RunRecordFileError::persistence("close synchronization", error))
                    .or_else(|| {
                        close
                            .err()
                            .map(|error| RunRecordFileError::persistence("close", error))
                    });
                if let Some(failure) = failure {
                    self.lifecycle = Lifecycle::Failed {
                        first: failure.clone(),
                    };
                    Err(failure)
                } else {
                    self.lifecycle = Lifecycle::Closed;
                    Ok(())
                }
            }
        }
    }

    /// Suppress repeated starts; every terminal callback writes its Run end.
    fn is_acknowledged(&self, boundary: &BoundaryAck) -> bool {
        match boundary {
            BoundaryAck::None | BoundaryAck::End => false,
            BoundaryAck::Start(run_id) => self.acknowledged_starts.contains(run_id),
        }
    }

    /// Insert one boundary only after every required persistence action succeeds.
    fn acknowledge(&mut self, boundary: BoundaryAck) {
        match boundary {
            BoundaryAck::None | BoundaryAck::End => {}
            BoundaryAck::Start(run_id) => {
                self.acknowledged_starts.insert(run_id);
            }
        }
    }
}

/// Require storage synchronization only before acknowledging a run end.
fn sync_required(boundary: &BoundaryAck) -> bool {
    matches!(boundary, BoundaryAck::End)
}

/// Complete write and flush, then optionally synchronize before returning.
fn persist(
    writer: &mut dyn RecordFileWriter,
    bytes: &[u8],
    sync: bool,
) -> Result<(), RunRecordFileError> {
    write_all(writer, bytes)?;
    writer
        .flush()
        .map_err(|error| RunRecordFileError::persistence("flush", error))?;
    if sync {
        writer
            .sync()
            .map_err(|error| RunRecordFileError::persistence("synchronization", error))?;
    }
    Ok(())
}

/// Drive partial writes until complete while preserving the first I/O failure.
fn write_all(
    writer: &mut dyn RecordFileWriter,
    mut bytes: &[u8],
) -> Result<(), RunRecordFileError> {
    while !bytes.is_empty() {
        match writer.write(bytes) {
            Ok(0) => {
                return Err(RunRecordFileError::persistence(
                    "write",
                    io::Error::new(io::ErrorKind::WriteZero, "writer made no progress"),
                ));
            }
            Ok(written) => bytes = &bytes[written..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(RunRecordFileError::persistence("write", error)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io;
    use std::sync::{Arc, Mutex};

    use super::{BoundaryAck, LogState, RecordFileWriter};
    use crate::run_record_file::RunRecordFileErrorKind;

    #[derive(Default)]
    /// Shared observations from one scripted writer.
    struct Probe {
        bytes: Vec<u8>,
        writes: usize,
        flushes: usize,
        syncs: usize,
        closes: usize,
    }

    /// One partial success or terminal write failure.
    enum WriteStep {
        Count(usize),
        Fail,
    }

    /// Deterministic writer outcomes consumed through the production seam.
    struct ScriptedWriter {
        probe: Arc<Mutex<Probe>>,
        writes: VecDeque<WriteStep>,
        fail_flush: bool,
        fail_sync: bool,
        fail_close: bool,
    }

    impl RecordFileWriter for ScriptedWriter {
        /// Consume one scripted write outcome and record reached bytes.
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let step = self
                .writes
                .pop_front()
                .unwrap_or(WriteStep::Count(bytes.len()));
            let mut probe = self.probe.lock().unwrap();
            probe.writes += 1;
            match step {
                WriteStep::Count(count) => {
                    let count = count.min(bytes.len());
                    probe.bytes.extend_from_slice(&bytes[..count]);
                    Ok(count)
                }
                WriteStep::Fail => Err(io::Error::other("scripted write failure")),
            }
        }

        /// Record and optionally fail one flush.
        fn flush(&mut self) -> io::Result<()> {
            self.probe.lock().unwrap().flushes += 1;
            if self.fail_flush {
                Err(io::Error::other("scripted flush failure"))
            } else {
                Ok(())
            }
        }

        /// Record and optionally fail one synchronization.
        fn sync(&mut self) -> io::Result<()> {
            self.probe.lock().unwrap().syncs += 1;
            if self.fail_sync {
                Err(io::Error::other("scripted sync failure"))
            } else {
                Ok(())
            }
        }

        /// Record and optionally fail one close.
        fn close(&mut self) -> io::Result<()> {
            self.probe.lock().unwrap().closes += 1;
            if self.fail_close {
                Err(io::Error::other("scripted close failure"))
            } else {
                Ok(())
            }
        }
    }

    /// Build one production state over scripted writer outcomes.
    fn state(
        writes: impl IntoIterator<Item = WriteStep>,
        fail_flush: bool,
        fail_sync: bool,
        fail_close: bool,
    ) -> (LogState, Arc<Mutex<Probe>>) {
        let probe = Arc::new(Mutex::new(Probe::default()));
        let writer = ScriptedWriter {
            probe: Arc::clone(&probe),
            writes: writes.into_iter().collect(),
            fail_flush,
            fail_sync,
            fail_close,
        };
        (LogState::active(Box::new(writer)), probe)
    }

    #[test]
    fn partial_writes_finish_before_flush_and_acknowledgement() {
        let (mut state, probe) = state([WriteStep::Count(2)], false, false, false);
        state
            .record(b"line\n", BoundaryAck::Start("run_a".to_owned()))
            .unwrap();
        state
            .record(b"ignored\n", BoundaryAck::Start("run_a".to_owned()))
            .unwrap();
        let probe = probe.lock().unwrap();
        assert_eq!(probe.bytes, b"line\n");
        assert_eq!(probe.writes, 2);
        assert_eq!(probe.flushes, 1);
        assert_eq!(probe.syncs, 0);
    }

    #[test]
    fn first_write_failure_disables_all_later_io_and_chains_unavailability() {
        let (mut state, probe) = state([WriteStep::Count(2), WriteStep::Fail], false, false, false);
        let first = state.record(b"line\n", BoundaryAck::None).unwrap_err();
        assert_eq!(first.kind(), RunRecordFileErrorKind::Persistence);
        let later = state.record(b"later\n", BoundaryAck::None).unwrap_err();
        assert_eq!(later.kind(), RunRecordFileErrorKind::Unavailable);
        assert!(std::error::Error::source(&later).is_some());
        assert_eq!(probe.lock().unwrap().writes, 2);
        state.close().unwrap();
        state.close().unwrap();
        assert_eq!(probe.lock().unwrap().syncs, 0);
    }

    #[test]
    fn flush_and_end_sync_failures_disable_before_acknowledgement() {
        let (mut flush_state, flush_probe) = state([], true, false, false);
        assert!(
            flush_state
                .record(b"start\n", BoundaryAck::Start("run_a".to_owned()))
                .is_err()
        );
        assert!(
            flush_state
                .record(b"start\n", BoundaryAck::Start("run_a".to_owned()))
                .is_err()
        );
        assert_eq!(flush_probe.lock().unwrap().writes, 1);

        let (mut sync_state, sync_probe) = state([], false, true, false);
        assert!(sync_state.record(b"end\n", BoundaryAck::End).is_err());
        assert!(sync_state.record(b"end\n", BoundaryAck::End).is_err());
        let probe = sync_probe.lock().unwrap();
        assert_eq!(probe.writes, 1);
        assert_eq!(probe.flushes, 1);
        assert_eq!(probe.syncs, 1);
    }

    #[test]
    fn healthy_close_syncs_and_closes_once_while_repeated_close_is_idempotent() {
        let (mut state, probe) = state([], false, false, false);
        state.close().unwrap();
        state.close().unwrap();
        let probe = probe.lock().unwrap();
        assert_eq!(probe.syncs, 1);
        assert_eq!(probe.closes, 1);
    }

    #[test]
    fn close_retains_only_its_first_new_failure_and_does_not_replay_it() {
        let (mut state, probe) = state([], false, true, true);
        let first = state.close().unwrap_err();
        assert_eq!(first.kind(), RunRecordFileErrorKind::Persistence);
        state.close().unwrap();
        let later = state.record(b"later\n", BoundaryAck::None).unwrap_err();
        assert_eq!(later.kind(), RunRecordFileErrorKind::Unavailable);
        assert!(std::error::Error::source(&later).is_some());
        let probe = probe.lock().unwrap();
        assert_eq!(probe.writes, 0);
        assert_eq!(probe.syncs, 1);
        assert_eq!(probe.closes, 1);
    }
}

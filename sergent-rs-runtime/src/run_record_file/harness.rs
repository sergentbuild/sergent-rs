//! Public harness creation, direct recording, observer boundaries, and close.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde_json::json;
use sergent_rs_core::error::RunError;
use sergent_rs_core::run_record::SergentResult;
use sergent_rs_core::timing::Timestamp;
use sergent_rs_core::vocab::{ProgressStatus, Stage};

use crate::clock::sample;
use crate::observer::RunObserver;
use crate::progress::ProgressSnapshot;

use super::encoding::{APP_ACTIVITY, SERGENT_ACTIVITY, USER_ACTIVITY, line};
use super::event::{RunRecordCorrelation, RunRecordEvent};
use super::identity::{RunRecordApplicationName, RunRecordFileId, path_for};
use super::writer::{BoundaryAck, FileWriter, LogState};
use super::{RunRecordFileError, writer::RecordFileWriter};

/// A shared, exact, opt-in persistence harness for sensitive run evidence.
/// @sergent/docs/run-record-file-format.md
pub struct JsonlRunRecordWriter {
    state: Mutex<LogState>,
    timestamp: fn() -> Timestamp,
}

impl JsonlRunRecordWriter {
    /// Create the exact harness-owned filename exclusively and return its path.
    pub fn create(
        directory: impl AsRef<Path>,
        application: RunRecordApplicationName,
        log_id: Option<RunRecordFileId>,
    ) -> Result<(Self, PathBuf), RunRecordFileError> {
        std::fs::create_dir_all(directory.as_ref())
            .map_err(|error| RunRecordFileError::creation(directory.as_ref(), error))?;
        let timestamp = sample().timestamp();
        Self::create_at(directory.as_ref(), application, log_id, timestamp)
    }

    /// Record one validated application event and surface any direct failure.
    pub fn record_app(&self, event: RunRecordEvent) -> Result<(), RunRecordFileError> {
        self.record_direct(APP_ACTIVITY, event)
    }

    /// Record one validated user event and surface any direct failure.
    pub fn record_user(&self, event: RunRecordEvent) -> Result<(), RunRecordFileError> {
        self.record_direct(USER_ACTIVITY, event)
    }

    /// Synchronize and close a healthy file once; repeated close is idempotent.
    pub fn close(&self) -> Result<(), RunRecordFileError> {
        self.lock().close()
    }

    /// Create one exact path from already sampled identity and time facts.
    fn create_at(
        directory: &Path,
        application: RunRecordApplicationName,
        log_id: Option<RunRecordFileId>,
        timestamp: sergent_rs_core::timing::Timestamp,
    ) -> Result<(Self, PathBuf), RunRecordFileError> {
        let log_id = log_id.unwrap_or_else(RunRecordFileId::mint);
        let path = path_for(directory, &application, &log_id, timestamp);
        let writer = FileWriter::create(&path)
            .map_err(|error| RunRecordFileError::creation(&path, error))?;
        Ok((Self::from_writer(Box::new(writer)), path))
    }

    /// Convert and encode a direct event fully before acquiring writer authority.
    fn record_direct(
        &self,
        activity: &'static str,
        event: RunRecordEvent,
    ) -> Result<(), RunRecordFileError> {
        let (name, payload, correlation) = event.into_json()?;
        let bytes = line(self.timestamp(), activity, &name, payload, &correlation)?;
        self.lock().record(&bytes, BoundaryAck::None)
    }

    /// Persist one started snapshot through start acknowledgement.
    fn write_start(&self, progress: &ProgressSnapshot) -> Result<(), RunRecordFileError> {
        let snapshot = serde_json::to_value(progress).map_err(|error| {
            RunRecordFileError::conversion(format!("run-start snapshot conversion failed: {error}"))
        })?;
        let correlation = RunRecordCorrelation::from_progress(progress);
        let bytes = line(
            self.timestamp(),
            SERGENT_ACTIVITY,
            "run.start",
            json!({ "snapshot": snapshot }),
            &correlation,
        )?;
        self.lock().record(
            &bytes,
            BoundaryAck::Start(progress.run_id.as_str().to_owned()),
        )
    }

    /// Persist one complete record through synchronized end acknowledgement.
    fn write_end<Scene>(&self, result: &SergentResult<Scene>) -> Result<(), RunRecordFileError> {
        let record = result.run_record();
        let run_record = serde_json::to_value(record).map_err(|error| {
            RunRecordFileError::conversion(format!("run-end record conversion failed: {error}"))
        })?;
        let scene = record.scene();
        let revision = scene
            .revision_after()
            .unwrap_or_else(|| scene.revision_before());
        let correlation =
            RunRecordCorrelation::run_scene(record.run_id(), scene.scene_id(), revision);
        let bytes = line(
            self.timestamp(),
            SERGENT_ACTIVITY,
            "run.end",
            json!({ "run_record": run_record }),
            &correlation,
        )?;
        self.lock().record(&bytes, BoundaryAck::End)
    }

    /// Bind one production or scripted writer to the single guarded lifecycle.
    fn from_writer(writer: Box<dyn RecordFileWriter>) -> Self {
        Self::from_writer_with_clock(writer, current_timestamp)
    }

    /// Bind one deterministic timestamp source to the writer lifecycle.
    pub(in crate::run_record_file) fn from_writer_with_clock(
        writer: Box<dyn RecordFileWriter>,
        timestamp: fn() -> Timestamp,
    ) -> Self {
        Self {
            state: Mutex::new(LogState::active(writer)),
            timestamp,
        }
    }

    /// Sample the line timestamp only after conversion succeeds.
    fn timestamp(&self) -> Timestamp {
        (self.timestamp)()
    }

    /// Acquire the sole writer guard while preserving contained callback behavior.
    fn lock(&self) -> MutexGuard<'_, LogState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl<Scene> RunObserver<Scene> for JsonlRunRecordWriter {
    /// Persist only the running Started snapshot and ignore all other progress.
    fn on_progress(&self, progress: &ProgressSnapshot) -> Result<(), RunError> {
        if progress.stage == Stage::Started && progress.status == ProgressStatus::Running {
            self.write_start(progress).map_err(observer_error)?;
        }
        Ok(())
    }

    /// Persist the one closed Run Record delivered to this observer slot.
    fn on_finished(&self, result: &SergentResult<Scene>) -> Result<(), RunError> {
        self.write_end(result).map_err(observer_error)
    }
}

impl Drop for JsonlRunRecordWriter {
    /// Best-effort synchronize a healthy harness when no explicit close occurred.
    fn drop(&mut self) {
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        drop(state.close());
    }
}

/// Adapt typed direct failure into the observer seam's ordinary returned error.
fn observer_error(error: RunRecordFileError) -> RunError {
    RunError::new("run_record_file_error", error.to_string())
}

/// Sample one production line timestamp from the runtime clock owner.
fn current_timestamp() -> Timestamp {
    sample().timestamp()
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use sergent_rs_core::ids::{RunId, SceneId};
    use sergent_rs_core::vocab::{ProgressStatus, Stage};

    use super::JsonlRunRecordWriter;
    use crate::observer::RunObserver;
    use crate::progress::ProgressSnapshot;
    use crate::run_record_file::writer::RecordFileWriter;
    use crate::run_record_file::{
        RunRecordApplicationName, RunRecordCorrelation, RunRecordEvent, RunRecordFileErrorKind,
        RunRecordFileId,
    };

    /// A writer that exposes direct and observer failure delivery without bytes.
    struct FailingWriter {
        writes: Arc<AtomicUsize>,
    }

    impl RecordFileWriter for FailingWriter {
        /// Count and fail the one reached write.
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            self.writes.fetch_add(1, Ordering::Relaxed);
            Err(io::Error::other("direct failure"))
        }

        /// Flush is unreachable after write failure.
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }

        /// Synchronization is unreachable after write failure.
        fn sync(&mut self) -> io::Result<()> {
            Ok(())
        }

        /// Dropping failed writer authority requires no scripted close action.
        fn close(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn fixed_creation_refuses_the_same_exact_file_without_truncating_it() {
        let directory = std::env::temp_dir();
        let app = RunRecordApplicationName::parse(format!("test{}", uuid::Uuid::new_v4().simple()))
            .unwrap();
        let id = RunRecordFileId::parse("fixed").unwrap();
        let timestamp = sergent_rs_core::timing::Timestamp::from_unix_micros(1_000_000);
        let (first, path) =
            JsonlRunRecordWriter::create_at(&directory, app.clone(), Some(id.clone()), timestamp)
                .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        first.close().unwrap();
        let error = JsonlRunRecordWriter::create_at(&directory, app, Some(id), timestamp)
            .err()
            .expect("the second exclusive create must fail");
        assert_eq!(
            error.kind(),
            crate::run_record_file::RunRecordFileErrorKind::Creation
        );
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn direct_failure_is_visible_and_observer_reuse_performs_no_later_io() {
        let writes = Arc::new(AtomicUsize::new(0));
        let log = JsonlRunRecordWriter::from_writer(Box::new(FailingWriter {
            writes: Arc::clone(&writes),
        }));
        let first = log
            .record_app(
                RunRecordEvent::new("first", None, RunRecordCorrelation::default()).unwrap(),
            )
            .unwrap_err();
        assert_eq!(first.kind(), RunRecordFileErrorKind::Persistence);
        let later = log
            .record_app(
                RunRecordEvent::new("later", None, RunRecordCorrelation::default()).unwrap(),
            )
            .unwrap_err();
        assert_eq!(later.kind(), RunRecordFileErrorKind::Unavailable);
        assert!(std::error::Error::source(&later).is_some());

        let progress = ProgressSnapshot {
            run_id: RunId::parse("run_00000000000000000000000000000001").unwrap(),
            scene_id: Some(SceneId::parse("doc_00000000000000000000000000000000").unwrap()),
            stage: Stage::Started,
            status: ProgressStatus::Running,
            revision: 1,
        };
        let observer =
            <JsonlRunRecordWriter as RunObserver<()>>::on_progress(&log, &progress).unwrap_err();
        assert_eq!(observer.kind, "run_record_file_error");
        assert_eq!(writes.load(Ordering::Relaxed), 1);
        log.close().unwrap();
    }
}

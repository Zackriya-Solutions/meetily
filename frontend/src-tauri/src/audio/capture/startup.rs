//! Bound OS audio startup and discard results after their caller has left.
use anyhow::{anyhow, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::{oneshot, Semaphore};

struct CancelOnDrop(Option<Arc<AtomicBool>>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(cancelled) = &self.0 {
            cancelled.store(true, Ordering::Release);
        }
    }
}

pub(super) async fn bounded_start<T: Send + 'static>(
    gate: Arc<Semaphore>,
    limit: Duration,
    start: impl FnOnce(Arc<AtomicBool>) -> Result<T> + Send + 'static,
) -> Result<T> {
    let permit = gate.try_acquire_owned().map_err(|_| anyhow!(
        "Audio device startup is still pending in macOS. Wait for it to finish, or restart Meetily before retrying."
    ))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut cancellation = CancelOnDrop(Some(cancelled.clone()));
    let (sender, receiver) = oneshot::channel();
    // A dedicated thread avoids runtime shutdown waiting indefinitely for a
    // blocked spawn_blocking task. The gate limits outstanding native starts.
    std::thread::Builder::new()
        .name("meetily-audio-start".into())
        .spawn(move || {
            let result = start(cancelled.clone());
            if cancelled.load(Ordering::Acquire) || sender.is_closed() {
                drop(result);
                drop(permit);
            } else {
                drop(permit);
                // A caller cancelled between the check and send owns no result;
                // send returns it here for disposal on this thread.
                let _ = sender.send(result);
            }
        })
        .map_err(|error| anyhow!("Could not start the audio device worker: {error}"))?;
    let result = tokio::time::timeout(limit, receiver).await
        .map_err(|_| anyhow!(
            "macOS audio device startup timed out. Check System Settings > Privacy & Security > Screen & System Audio Recording, then retry. If startup is still pending, restart Meetily."
        ))?
        .map_err(|_| anyhow!("The audio device startup worker exited unexpectedly. Retry recording."))?;
    cancellation.0 = None;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Resource(Option<oneshot::Sender<()>>);
    impl Drop for Resource {
        fn drop(&mut self) {
            let _ = self.0.take().unwrap().send(());
        }
    }

    #[tokio::test]
    async fn audio_startup_timeout_discards_late_result_and_bounds_retries() {
        let gate = Arc::new(Semaphore::new(1));
        let (release, wait) = std::sync::mpsc::channel();
        let (dropped, drop_result) = oneshot::channel();
        let result = bounded_start(gate.clone(), Duration::from_millis(20), move |cancelled| {
            wait.recv().unwrap();
            assert!(cancelled.load(Ordering::Acquire));
            Ok(Resource(Some(dropped)))
        })
        .await;
        assert!(result.err().unwrap().to_string().contains("timed out"));
        let retry = bounded_start(gate.clone(), Duration::from_secs(1), |_| Ok(())).await;
        assert!(retry.unwrap_err().to_string().contains("still pending"));
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), drop_result)
            .await
            .unwrap()
            .unwrap();
        let permit = tokio::time::timeout(Duration::from_secs(1), gate.clone().acquire_owned())
            .await
            .unwrap()
            .unwrap();
        drop(permit);
        bounded_start(gate, Duration::from_secs(1), |_| Ok(()))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn audio_startup_cancellation_disposes_result_without_handoff() {
        let gate = Arc::new(Semaphore::new(1));
        let (entered, started) = oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let (dropped, drop_result) = oneshot::channel();
        let task = tokio::spawn(bounded_start(
            gate,
            Duration::from_secs(1),
            move |cancelled| {
                let _ = entered.send(());
                wait.recv().unwrap();
                assert!(cancelled.load(Ordering::Acquire));
                Ok(Resource(Some(dropped)))
            },
        ));
        started.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), drop_result)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn audio_startup_preserves_success_and_native_error() {
        let gate = Arc::new(Semaphore::new(1));
        assert_eq!(
            bounded_start(gate.clone(), Duration::from_secs(1), |_| Ok(42))
                .await
                .unwrap(),
            42
        );
        let error = bounded_start::<()>(gate, Duration::from_secs(1), |_| {
            Err(anyhow!("Permission denied"))
        })
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "Permission denied");
    }
}

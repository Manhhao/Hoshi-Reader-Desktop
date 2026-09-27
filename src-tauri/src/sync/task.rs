use std::future::Future;

use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::sync::client::GoogleDriveError;

tokio::task_local! {
    static CURRENT: CancellationToken;
}

pub fn current() -> Option<CancellationToken> {
    CURRENT.try_with(CancellationToken::clone).ok()
}

pub fn is_cancelled() -> bool {
    current().is_some_and(|token| token.is_cancelled())
}

pub fn check_cancellation() -> Result<(), GoogleDriveError> {
    if is_cancelled() {
        return Err(GoogleDriveError::Cancelled);
    }
    Ok(())
}

pub async fn sleep(seconds: u64) -> Result<(), GoogleDriveError> {
    let token = current().unwrap_or_default();
    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_secs(seconds)) => Ok(()),
        _ = token.cancelled() => Err(GoogleDriveError::Cancelled),
    }
}

#[derive(Clone)]
pub struct SyncTask {
    token: CancellationToken,
    start: watch::Sender<bool>,
    done: watch::Receiver<bool>,
}

impl SyncTask {
    pub fn spawn<F>(future: F) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let token = CancellationToken::new();
        let (sender, done) = watch::channel(false);
        let (start, mut started) = watch::channel(false);
        tauri::async_runtime::spawn(CURRENT.scope(token.clone(), async move {
            if started.wait_for(|started| *started).await.is_ok() {
                future.await;
            }
            sender.send(true).ok();
        }));
        SyncTask { token, start, done }
    }

    pub fn start(&self) {
        self.start.send(true).ok();
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    pub async fn value(&self) {
        let mut done = self.done.clone();
        done.wait_for(|done| *done).await.ok();
    }
}

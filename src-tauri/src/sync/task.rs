use std::future::Future;
use std::sync::Arc;

use tokio::sync::{Semaphore, watch};
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

pub async fn concurrent<F, E>(futures: Vec<F>) -> Result<(), E>
where
    F: Future<Output = Result<(), E>> + Send + 'static,
    E: Send + 'static,
{
    let token = current().unwrap_or_default();
    let limit = Arc::new(Semaphore::new(8));
    let handles: Vec<_> = futures
        .into_iter()
        .map(|future| {
            let limit = limit.clone();
            tauri::async_runtime::spawn(CURRENT.scope(token.clone(), async move {
                let Ok(_permit) = limit.acquire().await else {
                    return Ok(());
                };
                let result = future.await;
                if result.is_err() {
                    limit.close();
                }
                result
            }))
        })
        .collect();
    let mut result = Ok(());
    for handle in handles {
        result = result.and(handle.await.unwrap());
    }
    result
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

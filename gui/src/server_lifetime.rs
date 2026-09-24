#[cfg(any(windows, test))]
use anyhow::{Result, bail};
use std::process::Child;
#[cfg(any(windows, test))]
use std::time::Duration;

/// Owns only the server spawned by this desktop. Dropping its stdin pipe also
/// tells the server to stop if the desktop process crashes or is terminated.
pub struct ManagedServer {
    pub child: Child,
}

impl ManagedServer {
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    #[cfg(any(windows, test))]
    pub async fn shutdown(&mut self) -> Result<()> {
        drop(self.child.stdin.take());
        for _ in 0..120 {
            if self.child.try_wait()?.is_some() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        bail!("로컬 서버가 종료 중입니다. 잠시 후 다시 종료하세요.")
    }
}

// A failed startup must not leave an untracked background server behind.
pub struct StartingServer(pub Option<Child>);
impl Drop for StartingServer {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

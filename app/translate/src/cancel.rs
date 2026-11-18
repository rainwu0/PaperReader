//! 取消的旗標：可以複製、可以在等待時被叫醒。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

#[derive(Debug, Clone, Default)]
pub struct Cancel {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    flag: AtomicBool,
    notify: Notify,
}

impl Cancel {
    pub fn new() -> Cancel {
        Cancel::default()
    }

    pub fn cancel(&self) {
        self.inner.flag.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.flag.load(Ordering::SeqCst)
    }

    /// 取消之後才完成。
    pub async fn cancelled(&self) {
        loop {
            let waiting = self.inner.notify.notified();
            tokio::pin!(waiting);
            // 先登記等待、再看旗標，才不會漏掉中間的 `cancel()`
            waiting.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            waiting.await;
        }
    }
}

//! Helpers for forwarding streams to channels.

use crate::channel::mpsc;
use crate::channel::mpsc::SendError;
use crate::cx::Cx;
use crate::runtime::yield_now;
use crate::stream::{Stream, StreamExt};

/// Sink wrapper for mpsc sender.
pub struct SinkStream<T> {
    sender: mpsc::Sender<T>,
}

impl<T> SinkStream<T> {
    /// Create a new SinkStream.
    #[must_use]
    pub fn new(sender: mpsc::Sender<T>) -> Self {
        Self { sender }
    }

    /// Send item through the channel.
    pub async fn send(&self, cx: &Cx, item: T) -> Result<(), SendError<T>> {
        self.sender.send(cx, item).await
    }

    /// Send all items from stream.
    pub async fn send_all<S>(&self, cx: &Cx, stream: S) -> Result<(), SendError<S::Item>>
    where
        S: Stream<Item = T> + Unpin,
    {
        forward(cx, stream, self.sender.clone()).await
    }
}

/// Convert a stream into a channel sender.
#[must_use]
pub fn into_sink<T>(sender: mpsc::Sender<T>) -> SinkStream<T> {
    SinkStream::new(sender)
}

/// Forward stream to channel.
pub async fn forward<S, T>(
    cx: &Cx,
    mut stream: S,
    sender: mpsc::Sender<T>,
) -> Result<(), SendError<T>>
where
    S: Stream<Item = T> + Unpin,
{
    while let Some(item) = stream.next().await {
        // Use try_send + yield_now to avoid blocking the executor
        // In Phase 0/1, we might not have async blocking send that yields to executor properly
        // so we spin with yield_now().
        let mut pending_item = item;
        loop {
            match sender.try_send(pending_item) {
                Ok(()) => break,
                Err(SendError::Full(val)) => {
                    pending_item = val;
                    // Check cancellation before yielding
                    if let Err(_e) = cx.checkpoint() {
                        return Err(SendError::Disconnected(pending_item));
                    }
                    yield_now().await;
                }
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::iter;
    use std::sync::Arc;
    use std::task::{Context, Wake, Waker};

    struct NoopWaker;

    impl Wake for NoopWaker {
        fn wake(self: Arc<Self>) {}
    }

    fn noop_waker() -> Waker {
        Waker::from(Arc::new(NoopWaker))
    }

    fn init_test(name: &str) {
        crate::test_utils::init_test_logging();
        crate::test_phase!(name);
    }

    /// Invariant: `into_sink` wraps an mpsc::Sender in a SinkStream.
    #[test]
    fn into_sink_creates_sink_stream() {
        init_test("into_sink_creates_sink_stream");
        let (tx, _rx) = mpsc::channel::<i32>(4);
        let _sink = into_sink(tx);
        // Construction succeeded — SinkStream wraps the sender.
        crate::test_complete!("into_sink_creates_sink_stream");
    }

    /// Invariant: `forward` delivers all stream items to the channel.
    #[test]
    fn forward_sends_all_items() {
        init_test("forward_sends_all_items");
        let cx: Cx = Cx::for_testing();
        let (tx, mut rx) = mpsc::channel::<i32>(8);
        let stream = iter(vec![10, 20, 30]);

        let mut future = std::pin::pin!(forward(&cx, stream, tx));
        let waker = noop_waker();
        let mut task_cx = Context::from_waker(&waker);

        // iter() yields synchronously, channel has capacity — should complete in one poll.
        let poll = future.as_mut().poll(&mut task_cx);
        let completed = matches!(poll, std::task::Poll::Ready(Ok(())));
        crate::assert_with_log!(completed, "forward completes", true, completed);

        // All items should be in the channel.
        let v1 = rx.try_recv();
        let ok1 = matches!(v1, Ok(10));
        crate::assert_with_log!(ok1, "received 10", true, ok1);
        let v2 = rx.try_recv();
        let ok2 = matches!(v2, Ok(20));
        crate::assert_with_log!(ok2, "received 20", true, ok2);
        let v3 = rx.try_recv();
        let ok3 = matches!(v3, Ok(30));
        crate::assert_with_log!(ok3, "received 30", true, ok3);

        crate::test_complete!("forward_sends_all_items");
    }

    /// Invariant: forwarding an empty stream completes immediately with Ok.
    #[test]
    fn forward_empty_stream_ok() {
        init_test("forward_empty_stream_ok");
        let cx: Cx = Cx::for_testing();
        let (tx, _rx) = mpsc::channel::<i32>(4);
        let stream = iter(Vec::<i32>::new());

        let mut future = std::pin::pin!(forward(&cx, stream, tx));
        let waker = noop_waker();
        let mut task_cx = Context::from_waker(&waker);

        let poll = future.as_mut().poll(&mut task_cx);
        let completed = matches!(poll, std::task::Poll::Ready(Ok(())));
        crate::assert_with_log!(completed, "empty forward completes", true, completed);

        crate::test_complete!("forward_empty_stream_ok");
    }
}

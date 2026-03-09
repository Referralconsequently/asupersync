//! Map combinator for streams.
//!
//! The `Map` combinator transforms each item in a stream using a provided function.

use super::Stream;
use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A stream that transforms each item using a function.
///
/// Created by [`StreamExt::map`](super::StreamExt::map).
#[derive(Debug)]
#[must_use = "streams do nothing unless polled"]
#[pin_project]
pub struct Map<S, F> {
    #[pin]
    stream: S,
    f: F,
}

impl<S, F> Map<S, F> {
    /// Creates a new `Map` stream.
    pub(crate) fn new(stream: S, f: F) -> Self {
        Self { stream, f }
    }

    /// Returns a reference to the underlying stream.
    pub fn get_ref(&self) -> &S {
        &self.stream
    }

    /// Returns a mutable reference to the underlying stream.
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// Consumes the combinator, returning the underlying stream.
    pub fn into_inner(self) -> S {
        self.stream
    }
}

impl<S, F, T> Stream for Map<S, F>
where
    S: Stream,
    F: FnMut(S::Item) -> T,
{
    type Item = T;

    #[inline]
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.project();
        match this.stream.poll_next(cx) {
            Poll::Ready(Some(item)) => Poll::Ready(Some((this.f)(item))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::iter;
    use std::sync::Arc;
    use std::task::{Wake, Waker};

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

    #[test]
    fn map_transforms_items() {
        init_test("map_transforms_items");
        let mut stream = Map::new(iter(vec![1i32, 2, 3]), |x: i32| x * 2);
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        let poll = Pin::new(&mut stream).poll_next(&mut cx);
        let ok = matches!(poll, Poll::Ready(Some(2)));
        crate::assert_with_log!(ok, "poll 1", "Poll::Ready(Some(2))", poll);
        let poll = Pin::new(&mut stream).poll_next(&mut cx);
        let ok = matches!(poll, Poll::Ready(Some(4)));
        crate::assert_with_log!(ok, "poll 2", "Poll::Ready(Some(4))", poll);
        let poll = Pin::new(&mut stream).poll_next(&mut cx);
        let ok = matches!(poll, Poll::Ready(Some(6)));
        crate::assert_with_log!(ok, "poll 3", "Poll::Ready(Some(6))", poll);
        let poll = Pin::new(&mut stream).poll_next(&mut cx);
        let ok = matches!(poll, Poll::Ready(None));
        crate::assert_with_log!(ok, "poll done", "Poll::Ready(None)", poll);
        crate::test_complete!("map_transforms_items");
    }

    #[test]
    fn map_preserves_size_hint() {
        init_test("map_preserves_size_hint");
        let stream = Map::new(iter(vec![1i32, 2, 3]), |x: i32| x * 2);
        let hint = stream.size_hint();
        let ok = hint == (3, Some(3));
        crate::assert_with_log!(ok, "size hint", (3, Some(3)), hint);
        crate::test_complete!("map_preserves_size_hint");
    }

    #[test]
    fn map_type_change() {
        init_test("map_type_change");
        let mut stream = Map::new(iter(vec![1i32, 2, 3]), |x: i32| x.to_string());
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        let poll = Pin::new(&mut stream).poll_next(&mut cx);
        let ok = matches!(poll, Poll::Ready(Some(ref s)) if s == "1");
        crate::assert_with_log!(ok, "poll 1", "Poll::Ready(Some(\"1\"))", poll);
        crate::test_complete!("map_type_change");
    }

    /// Invariant: map of empty stream produces None immediately.
    #[test]
    fn map_empty_stream() {
        init_test("map_empty_stream");
        let mut stream = Map::new(iter(Vec::<i32>::new()), |x: i32| x * 2);
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        let poll = Pin::new(&mut stream).poll_next(&mut cx);
        let is_none = matches!(poll, Poll::Ready(None));
        crate::assert_with_log!(is_none, "empty map yields None", true, is_none);
        crate::test_complete!("map_empty_stream");
    }

    /// Invariant: Map accessors (get_ref, get_mut, into_inner) work correctly.
    #[test]
    fn map_accessors() {
        init_test("map_accessors");
        let mut stream = Map::new(iter(vec![1, 2, 3]), |x: i32| x + 10);

        let _inner_ref = stream.get_ref();
        let _inner_mut = stream.get_mut();

        let recovered = stream.into_inner();
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let mut recovered = recovered;
        let poll = Pin::new(&mut recovered).poll_next(&mut cx);
        let got_1 = matches!(poll, Poll::Ready(Some(1)));
        crate::assert_with_log!(got_1, "into_inner preserves items", true, got_1);

        crate::test_complete!("map_accessors");
    }

    #[test]
    fn map_debug() {
        fn double(x: i32) -> i32 {
            x * 2
        }
        let stream = Map::new(iter(vec![1, 2, 3]), double as fn(i32) -> i32);
        let dbg = format!("{stream:?}");
        assert!(dbg.contains("Map"));
    }
}

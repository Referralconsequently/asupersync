#![allow(missing_docs)]

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use asupersync::sync::Notify;

struct NoopWaker;
impl Wake for NoopWaker {
    fn wake(self: Arc<Self>) {}
    fn wake_by_ref(self: &Arc<Self>) {}
}

fn main() {
    let notify = Notify::new();
    let mut fut1 = notify.notified();
    let mut fut2 = notify.notified();

    let waker: Waker = Arc::new(NoopWaker).into();
    let mut cx = Context::from_waker(&waker);

    let _ = Pin::new(&mut fut1).poll(&mut cx);
    let _ = Pin::new(&mut fut2).poll(&mut cx);

    notify.notify_one();
    notify.notify_waiters();

    let ready1 = matches!(Pin::new(&mut fut1).poll(&mut cx), Poll::Ready(()));
    let ready2 = matches!(Pin::new(&mut fut2).poll(&mut cx), Poll::Ready(()));
    println!("ready1: {ready1}");
    println!("ready2: {ready2}");

    let mut fut3 = notify.notified();
    let ready3 = matches!(Pin::new(&mut fut3).poll(&mut cx), Poll::Ready(()));
    println!("ready3: {ready3}");
}

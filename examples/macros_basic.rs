#![allow(missing_docs)]
#![allow(
    clippy::trivially_copy_pass_by_ref,
    clippy::unnecessary_wraps,
    clippy::unused_self
)]

#[cfg(feature = "proc-macros")]
mod demo {
    use asupersync::{join, scope, spawn};
    use std::future::Future;
    use std::marker::PhantomData;

    #[derive(Clone, Copy)]
    struct MiniCx;

    struct MiniScope;
    struct MiniState;

    #[derive(Debug)]
    struct MiniError;

    struct MiniHandle<T>(PhantomData<T>);

    impl MiniCx {
        fn scope(&self) -> MiniScope {
            MiniScope
        }

        #[allow(dead_code)]
        fn scope_with_budget(&self, _budget: asupersync::Budget) -> MiniScope {
            MiniScope
        }
    }

    impl MiniScope {
        fn spawn_registered<F, Fut>(
            &self,
            _state: &mut MiniState,
            _cx: &MiniCx,
            f: F,
        ) -> Result<MiniHandle<Fut::Output>, MiniError>
        where
            F: FnOnce(MiniCx) -> Fut,
            Fut: Future,
        {
            std::mem::drop(f(MiniCx));
            Ok(MiniHandle(PhantomData))
        }
    }

    pub async fn demo() {
        let cx = MiniCx;
        let mut state = MiniState;
        let __state = &mut state;

        let (a, b) = join!(async { 1 }, async { 2 });

        let _ = scope!(cx, {
            let _handle = spawn!(async { 42 });
            (a, b)
        });
    }
}

#[cfg(feature = "proc-macros")]
fn main() {
    std::mem::drop(demo::demo());
}

#[cfg(not(feature = "proc-macros"))]
fn main() {}

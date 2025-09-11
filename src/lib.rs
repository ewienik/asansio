use futures::future;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::Hash;
use std::hash::Hasher;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Id(usize);

impl<'a> From<&'a Waker> for Id {
    fn from(waker: &'a Waker) -> Self {
        Self(waker as *const Waker as usize)
    }
}

struct State<T: SansIoMachine> {
    tasks: HashMap<Id, (&'static Waker, LocalBoxFuture<'a, ()>)>,
    new_tasks: Vec<LocalBoxFuture<'a, ()>>,
    request: Option<T::Request>,
    responses: HashMap<Id, T::Response>,
    inner: T,
}

#[derive(Clone)]
pub struct SansIo<'a, T: SansIoMachine> {}

pub trait SansIoMachine: Sized {
    type Request;
    type Response;
    type Error;

    async fn start(&self, sansio: &mut SansIo<Self>);
    async fn handle(&self, sansio: &mut SansIo<Self>, id: Id, response: Self::Response);
}

impl<'a, T: SansIoMachine> SansIo<'a, T> {
    pub fn new(inner: T) -> Self {
        Self {
            tasks: HashMap::new(),
            new_tasks: Vec::new(),
            request: None,
            responses: HashMap::new(),
            inner,
        }
    }

    fn run_async(&mut self, id: Id) -> Vec<(Id, T::Request)> {
        let mut requests = Vec::new();
        let mut queue: VecDeque<_> = [id].into_iter().collect();
        while let Some(id) = queue.pop_front() {
            let (waker, mut fut) = self.tasks.remove(&id).unwrap();
            let mut ctx = Context::from_waker(waker);
            if matches!(fut.poll_unpin(&mut ctx), Poll::Pending)
                && let Some(request) = self.request.take()
            {
                requests.push((id, request));
            }
        }
        requests
    }

    pub fn start(&mut self) -> Vec<(Id, T::Request)> {
        let waker = Waker::noop();
        let id = waker.into();
        self.tasks.insert(
            id,
            (
                waker,
                async |sansio: &mut SansIo<'_, T::Request, T::Response>| {
                    {
                        self.inner.start(self).await;
                    }
                    .boxed_local()
                },
            ),
        );
        if self.run_async(id) {
            return Vec::new();
        }
        Vec::new()
    }

    pub fn handle(
        &mut self,
        id: Id,
        response: T::Response,
    ) -> Result<Vec<T::Request>, SansIoError> {
        self.responses.insert(id, response);
        let requests = Vec::new();
        if id.0 >= self.tasks.len() {
            return Err(SansIoError::TaskNotExists);
        }
        Ok(requests)
    }

    pub fn spawn(&mut self, f: impl Future<Output = ()> + 'a) -> impl Future<Output = ()> {
        future::poll_fn(|ctx| {
            let id = ctx.waker().into();
            self.new_tasks.push(f.boxed_local());
            Poll::Ready(())
        })
    }

    pub fn call(&mut self, request: Request) -> impl Future<Output = Response> {
        self.request = Some(request);
        future::poll_fn(|ctx| {
            let id = ctx.waker().into();
            if let Some(response) = self.responses.borrow_mut().remove(&id) {
                Poll::Ready(response)
            } else {
                Poll::Pending
            }
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SansIoError {
    #[error("task not exists")]
    TaskNotExists,
}

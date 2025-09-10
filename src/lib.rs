use futures::future;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use std::cell::RefCell;
use std::collections::HashMap;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub struct Id(usize);

impl Id {
    fn first() -> Self {
        Self(0)
    }

    fn next(&mut self) -> Self {
        self.0 += 1;
        Self(self.0 - 1)
    }
}

pub struct SansIo<'a, T: SansIoMachine> {
    id: Id,
    tasks: HashMap<Id, LocalBoxFuture<'a, ()>>,
    responses: Rc<RefCell<HashMap<Id, T::Response>>>,
    inner: T,
}

pub struct SansIoHandler<'a, Request, Response> {
    id: Id,
    responses: Rc<RefCell<HashMap<Id, Response>>>,
    request: Option<Request>,
    tasks: Vec<LocalBoxFuture<'a, ()>>,
}

impl<'a, Request, Response> SansIoHandler<'a, Request, Response> {
    fn new(id: Id, responses: Rc<RefCell<HashMap<Id, Response>>>) -> Self {
        Self {
            id,
            responses,
            request: None,
            tasks: Vec::new(),
        }
    }

    pub fn spawn(&mut self, f: impl Future<Output = ()> + 'a) -> Result<(), SansIoError> {
        self.tasks.push(f.boxed_local());
        Ok(())
    }

    pub fn call(&mut self, request: Request) -> impl Future<Output = Response> {
        self.request = Some(request);
        let id = self.id;
        let responses = self.responses.clone();
        future::poll_fn(move |_| {
            if let Some(response) = responses.borrow_mut().remove(&id) {
                Poll::Ready(response)
            } else {
                Poll::Pending
            }
        })
    }
}

pub trait SansIoMachine {
    type Request;
    type Response;
    type Error;

    async fn start(&self, sansio: &mut SansIoHandler<Self::Request, Self::Response>);
    async fn handle(
        &self,
        sansio: &mut SansIoHandler<Self::Request, Self::Response>,
        id: Id,
        response: Self::Response,
    );
}

impl<'a, T: SansIoMachine> SansIo<'a, T> {
    pub fn new(inner: T) -> Self {
        Self {
            id: Id::first(),
            tasks: HashMap::new(),
            responses: Rc::new(RefCell::new(HashMap::new())),
            inner,
        }
    }

    pub fn start(&mut self) -> Vec<(Id, T::Request)> {
        let mut handler: SansIoHandler<'_, T::Request, _> =
            SansIoHandler::new(self.id.next(), self.responses.clone());
        let mut ctx = Context::from_waker(Waker::noop());
        let mut fut = self.inner.start(&mut handler).boxed_local();
        let result = fut.poll_unpin(&mut ctx);
        Vec::new()
    }

    pub fn handle(
        &mut self,
        id: Id,
        _response: T::Response,
    ) -> Result<Vec<T::Request>, SansIoError> {
        let requests = Vec::new();
        if id.0 >= self.tasks.len() {
            return Err(SansIoError::TaskNotExists);
        }
        Ok(requests)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SansIoError {
    #[error("task not exists")]
    TaskNotExists,
}

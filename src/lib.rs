use futures::future;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use std::cell::Cell;
use std::rc::Rc;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

enum TaskChannel<Request, Response> {
    Tx(Result<Request, SansIoError>),
    Rx(Response),
    None,
}

impl<Request, Response> Default for TaskChannel<Request, Response> {
    fn default() -> Self {
        TaskChannel::None
    }
}

pub struct Handler<Request, Response>(Rc<Cell<TaskChannel<Request, Response>>>);

impl<Request, Response> Clone for Handler<Request, Response> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<Request, Response> Handler<Request, Response> {
    pub fn call(&self, request: Request) -> impl Future<Output = Response> {
        match self.0.replace(TaskChannel::Tx(Ok(request))) {
            TaskChannel::Tx(_) => {
                self.0
                    .replace(TaskChannel::Tx(Err(SansIoError::RequestNotTaken)));
            }
            TaskChannel::Rx(_) => {
                self.0
                    .replace(TaskChannel::Tx(Err(SansIoError::ResponseNotTaken)));
            }
            TaskChannel::None => {}
        };
        let ch = self.0.clone();
        future::poll_fn(move |_| match ch.take() {
            TaskChannel::Tx(request) => {
                ch.replace(TaskChannel::Tx(request));
                Poll::Pending
            }
            TaskChannel::Rx(response) => Poll::Ready(response),
            TaskChannel::None => panic!("no request or response"),
        })
    }
}

pub struct SansIo<'a, Request, Response> {
    handler: Handler<Request, Response>,
    task: Option<LocalBoxFuture<'a, ()>>,
}

impl<'a, Request, Response> SansIo<'a, Request, Response> {
    pub fn new() -> Self {
        Self {
            handler: Handler(Rc::new(Cell::new(TaskChannel::default()))),
            task: None,
        }
    }

    fn run_async(&mut self) -> Result<Option<Request>, SansIoError> {
        let mut task = self.task.take().unwrap();
        let waker = Waker::noop();
        let mut ctx = Context::from_waker(waker);
        match task.poll_unpin(&mut ctx) {
            Poll::Ready(_) => Ok(None),
            Poll::Pending => {
                self.task = Some(task);
                let request = match self.handler.0.replace(TaskChannel::None) {
                    TaskChannel::Tx(request) => request,
                    TaskChannel::Rx(_) => panic!("response not taken"),
                    TaskChannel::None => panic!("no request"),
                };
                Some(request).transpose()
            }
        }
    }

    pub fn handler(&self) -> Handler<Request, Response> {
        self.handler.clone()
    }

    pub fn start(
        &mut self,
        f: impl Future<Output = ()> + 'a,
    ) -> Result<Option<Request>, SansIoError> {
        self.task = Some(f.boxed_local());
        self.run_async()
    }

    pub fn handle(&mut self, response: Response) -> Result<Option<Request>, SansIoError> {
        match self.handler.0.replace(TaskChannel::Rx(response)) {
            TaskChannel::Tx(_) => return Err(SansIoError::RequestNotTaken),
            TaskChannel::Rx(_) => return Err(SansIoError::ResponseNotTaken),
            TaskChannel::None => {}
        };
        self.run_async()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SansIoError {
    #[error("request not taken")]
    RequestNotTaken,
    #[error("response not taken")]
    ResponseNotTaken,
    #[error("no request")]
    NoRequest,
}

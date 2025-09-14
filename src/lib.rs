use core::marker::PhantomData;
use core::mem;
use core::pin::Pin;
use core::ptr;
use core::task::Context;
use core::task::Poll;
use core::task::RawWaker;
use core::task::RawWakerVTable;
use core::task::Waker;
use futures::future::LocalBoxFuture;
use futures::FutureExt;

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

pub struct Call<Request, Response> {
    request: Option<Request>,
    _response: PhantomData<Response>,
}

impl<Request: Unpin, Response: Unpin> Future for Call<Request, Response> {
    type Output = Response;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let waker = cx.waker();
        assert!(ptr::eq(waker.vtable(), &WAKER_VTABLE));
        let sansio = unsafe { &mut *(waker.data() as *mut SansIo<Request, Response>) };

        sansio.ch = TaskChannel::Tx(if self.request.is_none() {
            match mem::replace(&mut sansio.ch, TaskChannel::None) {
                TaskChannel::Tx(_) => Err(SansIoError::RequestNotTaken),
                TaskChannel::Rx(response) => return Poll::Ready(response),
                TaskChannel::None => Err(SansIoError::NoRequestOrResponse),
            }
        } else {
            match sansio.ch {
                TaskChannel::Tx(_) => Err(SansIoError::RequestNotTaken),
                TaskChannel::Rx(_) => Err(SansIoError::ResponseNotTaken),
                TaskChannel::None => Ok(self.request.take().unwrap()),
            }
        });
        Poll::Pending
    }
}

pub fn call<Request, Response>(request: Request) -> Call<Request, Response> {
    Call {
        request: Some(request),
        _response: PhantomData,
    }
}

pub struct SansIo<'a, Request, Response> {
    ch: TaskChannel<Request, Response>,
    task: Option<LocalBoxFuture<'a, ()>>,
}

impl<'a, Request, Response> SansIo<'a, Request, Response> {
    pub fn new() -> Self {
        Self {
            ch: TaskChannel::default(),
            task: None,
        }
    }

    fn run_async(&mut self) -> Result<Option<Request>, SansIoError> {
        let mut task = self.task.take().unwrap();
        let waker = unsafe { Waker::new(self as *const _ as *const (), &WAKER_VTABLE) };
        let mut ctx = Context::from_waker(&waker);
        match task.poll_unpin(&mut ctx) {
            Poll::Ready(_) => Ok(None),
            Poll::Pending => {
                self.task = Some(task);
                let request = match mem::replace(&mut self.ch, TaskChannel::None) {
                    TaskChannel::Tx(request) => request,
                    TaskChannel::Rx(_) => return Err(SansIoError::ResponseNotTaken),
                    TaskChannel::None => return Err(SansIoError::NoRequest),
                };
                Some(request).transpose()
            }
        }
    }

    pub fn start(
        &mut self,
        f: impl Future<Output = ()> + 'a,
    ) -> Result<Option<Request>, SansIoError> {
        self.task = Some(f.boxed_local());
        self.run_async()
    }

    pub fn handle(&mut self, response: Response) -> Result<Option<Request>, SansIoError> {
        match mem::replace(&mut self.ch, TaskChannel::Rx(response)) {
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
    #[error("no request or response")]
    NoRequestOrResponse,
}

const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |data| RawWaker::new(data, &WAKER_VTABLE),
    |_| {},
    |_| {},
    |_| {},
);

#![no_std]

use core::marker::PhantomData;
use core::mem;
use core::pin::Pin;
use core::ptr;
use core::task::Context;
use core::task::Poll;
use core::task::RawWaker;
use core::task::RawWakerVTable;
use core::task::Waker;

#[derive(Default)]
enum Channel<Request, Response> {
    Tx(*const Request),
    Rx(*const Response),
    #[default]
    None,
}

impl<Request, Response> Channel<Request, Response> {
    fn tx(request: &Request) -> Self {
        Self::Tx(request as *const Request)
    }

    fn rx(response: &Response) -> Self {
        Self::Rx(response as *const Response)
    }
}

pub struct Call<'a, Request, Response> {
    request: Option<&'a Request>,
    _response: PhantomData<Response>,
}

impl<'a, Request: Unpin, Response: Unpin> Future for Call<'a, Request, Response> {
    type Output = Handler<Response>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let waker = cx.waker();
        assert!(ptr::eq(waker.vtable(), &WAKER_VTABLE));
        let ch = unsafe { &mut *(waker.data() as *mut Channel<Request, Response>) };

        if let Some(request) = self.request.take() {
            *ch = Channel::tx(request);
            Poll::Pending
        } else {
            match ch {
                Channel::Rx(response) => Poll::Ready(Handler {
                    response: *response,
                }),
                Channel::Tx(_) => Poll::Pending,
                Channel::None => unreachable!(),
            }
        }
    }
}

pub struct Handler<Response> {
    response: *const Response,
}

impl<Response> Handler<Response> {
    pub fn new() -> Self {
        Self {
            response: ptr::null(),
        }
    }

    pub fn call<Request>(self, request: &Request) -> Call<'_, Request, Response> {
        Call {
            request: Some(request),
            _response: PhantomData,
        }
    }

    pub fn response(&self) -> Option<&Response> {
        if self.response.is_null() {
            return None;
        }
        Some(unsafe { &*self.response })
    }
}

pub struct SansIo<'a, Request, Task> {
    request: Option<&'a Request>,
    task: Pin<&'a mut Task>,
}

impl<'a, Request, Task> SansIo<'a, Request, Task>
where
    Task: Future<Output = ()>,
{
    pub fn start<Response>(task: Pin<&'a mut Task>) -> Option<Self> {
        let mut sansio = Self {
            request: None,
            task,
        };
        sansio
            .run_async(Channel::<Request, Response>::None)
            .then_some(sansio)
    }

    pub fn request(&self) -> Option<&'a Request> {
        self.request
    }

    pub fn handle<Response>(mut self, response: &Response) -> Option<Self> {
        self.run_async(Channel::rx(response)).then_some(self)
    }

    fn run_async<Response>(&mut self, mut ch: Channel<Request, Response>) -> bool {
        let waker = unsafe { Waker::new(&ch as *const _ as *const (), &WAKER_VTABLE) };
        let mut cx = Context::from_waker(&waker);
        self.request = match self.task.as_mut().poll(&mut cx) {
            Poll::Ready(_) => None,
            Poll::Pending => {
                let Channel::Tx(request) = mem::replace(&mut ch, Channel::None) else {
                    unreachable!();
                };
                Some(unsafe { &*request })
            }
        };
        self.request.is_some()
    }
}

const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |data| RawWaker::new(data, &WAKER_VTABLE),
    |_| {},
    |_| {},
    |_| {},
);

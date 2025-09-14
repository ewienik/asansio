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
    Tx(Request),
    Rx(Response),
    #[default]
    None,
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
        let ch = unsafe { &mut *(waker.data() as *mut Channel<Request, Response>) };

        if self.request.is_none() {
            let Channel::Rx(response) = mem::replace(ch, Channel::None) else {
                unreachable!();
            };
            Poll::Ready(response)
        } else {
            *ch = Channel::Tx(self.request.take().unwrap());
            Poll::Pending
        }
    }
}

pub fn call<Request, Response>(request: Request) -> Call<Request, Response> {
    Call {
        request: Some(request),
        _response: PhantomData,
    }
}

pub struct SansIo<'a, Request, Response, Task> {
    ch: Channel<Request, Response>,
    task: Pin<&'a mut Task>,
}

impl<'a, Request, Response, Task> SansIo<'a, Request, Response, Task>
where
    Task: Future<Output = ()>,
{
    pub fn start(task: Pin<&'a mut Task>) -> Option<(Self, Request)> {
        let mut sansio = Self {
            ch: Channel::default(),
            task,
        };
        let result = sansio.run_async();
        result.map(|result| (sansio, result))
    }

    pub fn handle(mut self, response: Response) -> Option<(Self, Request)> {
        self.ch = Channel::Rx(response);
        let result = self.run_async();
        result.map(|result| (self, result))
    }

    fn run_async(&mut self) -> Option<Request> {
        let waker = unsafe { Waker::new(&self.ch as *const _ as *const (), &WAKER_VTABLE) };
        let mut cx = Context::from_waker(&waker);
        match self.task.as_mut().poll(&mut cx) {
            Poll::Ready(_) => None,
            Poll::Pending => {
                let Channel::Tx(request) = mem::replace(&mut self.ch, Channel::None) else {
                    unreachable!();
                };
                Some(request)
            }
        }
    }
}

const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |data| RawWaker::new(data, &WAKER_VTABLE),
    |_| {},
    |_| {},
    |_| {},
);

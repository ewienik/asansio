#![no_std]

use core::marker::PhantomData;
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

pub struct SansHandle<'a, Request, Response> {
    request: Option<&'a Request>,
    _response: PhantomData<Response>,
}

impl<'a, Request: Unpin, Response: Unpin> Future for SansHandle<'a, Request, Response> {
    type Output = SansResponse<Response>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let waker = cx.waker();
        assert!(ptr::eq(waker.vtable(), &WAKER_VTABLE));
        let ch = unsafe { &mut *(waker.data() as *mut Channel<Request, Response>) };

        if let Some(request) = self.request.take() {
            *ch = Channel::tx(request);
            Poll::Pending
        } else {
            match ch {
                Channel::Rx(response) => Poll::Ready(SansResponse {
                    response: *response,
                }),
                Channel::Tx(_) => Poll::Pending,
                Channel::None => unreachable!(),
            }
        }
    }
}

pub struct Sans<Request, Response> {
    _request: PhantomData<Request>,
    _response: PhantomData<Response>,
}

pub struct SansResponse<Response> {
    response: *const Response,
}

unsafe impl<Response> Send for SansResponse<Response> {}

impl<Request, Response> Sans<Request, Response> {
    pub fn start<'a>(&self, request: &'a Request) -> SansHandle<'a, Request, Response> {
        SansHandle {
            request: Some(request),
            _response: PhantomData,
        }
    }

    pub fn handle<'a>(
        &self,
        _response: SansResponse<Response>,
        request: &'a Request,
    ) -> SansHandle<'a, Request, Response> {
        SansHandle {
            request: Some(request),
            _response: PhantomData,
        }
    }
}

impl<Response> SansResponse<Response> {
    pub fn response(&self) -> Option<&Response> {
        if self.response.is_null() {
            return None;
        }
        Some(unsafe { &*self.response })
    }
}

pub struct Io<Request, Response> {
    _request: PhantomData<Request>,
    _response: PhantomData<Response>,
}

pub struct IoRequest<'a, Request, Task> {
    request: Option<&'a Request>,
    task: Pin<&'a mut Task>,
}

impl<Request, Response> Io<Request, Response> {
    pub fn start<'a, 'b, Task>(
        &'a self,
        task: Pin<&'b mut Task>,
    ) -> Option<IoRequest<'b, Request, Task>>
    where
        Task: Future<Output = ()>,
    {
        let mut handler = IoRequest {
            request: None,
            task,
        };
        handler.run_async(Channel::<Request, Response>::None);
        handler.request.map(|_| handler)
    }

    pub fn handle<'a, Task>(
        &self,
        mut handler: IoRequest<'a, Request, Task>,
        response: &Response,
    ) -> Option<IoRequest<'a, Request, Task>>
    where
        Task: Future<Output = ()>,
    {
        handler.run_async(Channel::rx(response));
        handler.request.map(move |_| handler)
    }
}

impl<'a, Request, Task> IoRequest<'a, Request, Task>
where
    Task: Future<Output = ()>,
{
    pub fn request(&self) -> Option<&Request> {
        self.request
    }

    fn run_async<Response>(&mut self, ch: Channel<Request, Response>) {
        let waker = unsafe { Waker::new(&ch as *const _ as *const (), &WAKER_VTABLE) };
        let mut cx = Context::from_waker(&waker);
        self.request = match self.task.as_mut().poll(&mut cx) {
            Poll::Ready(_) => None,
            Poll::Pending => {
                let Channel::Tx(request) = ch else {
                    unreachable!();
                };
                Some(unsafe { &*request })
            }
        }
    }
}

pub fn new<Request, Response>() -> (Sans<Request, Response>, Io<Request, Response>) {
    (
        Sans {
            _request: PhantomData,
            _response: PhantomData,
        },
        Io {
            _request: PhantomData,
            _response: PhantomData,
        },
    )
}

const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |data| RawWaker::new(data, &WAKER_VTABLE),
    |_| {},
    |_| {},
    |_| {},
);

//! # asansio
//!
//! This library contains the async/await state machine for the sans-io design pattern. See [sans
//! I/O for network protocols](https://sans-io.readthedocs.io/) documentation to familiar with the
//! concept.  Writing network protocol without performing I/O operations means creating a state
//! machine.  Manually creating a state machine could be a tedious process. As Rust async/await
//! concept is an actual state machine with implicit states created during compilation, this
//! library is an experiment with using async/await to provide such state machine automatically.
//! Let's check if this is good solution to the problem.
//!
//! This crate could be used also for non network protocol cases, everywhere there is a need for
//! creating a state machine.
//!
//! This is `no_std` crate and it doesn't allocate on the heap.
//!
//! ## Usage
//!
//! See this simple example:
//!
//! ```
//! # use asansio::Sans;
//! # use std::pin::pin;
//! #
//! struct Request<'a>(&'a [u8]);
//! struct Response<'a>(&'a [u8]);
//!
//! async fn sans_task<'a>(sans: Sans<Request<'a>, Response<'a>>) {
//!     let mut request_buf = [1u8; 10];
//!     let response = sans.start(&Request(&request_buf)).await;
//!     assert_eq!(response.response().unwrap().0, [2; 20]);
//!
//!     request_buf.fill(3);
//!     let response = sans.handle(response, &Request(&request_buf)).await;
//!     assert_eq!(response.response().unwrap().0, [4; 20]);
//! }
//!
//! let (sans, io) = asansio::new();
//!
//! let task = pin!(sans_task(sans));
//!
//! let request = io.start(task).unwrap();
//! assert_eq!(request.request().unwrap().0, [1; 10]);
//!
//! let mut response_buf = [2; 20];
//! let request = io.handle(request, &Response(&response_buf)).unwrap();
//! assert_eq!(request.request().unwrap().0, [3; 10]);
//!
//! response_buf.fill(4);
//! assert!(io.handle(request, &Response(&response_buf)).is_none());
//! ```
//!
//! This crate divides a problem into two parts. The first `Sans` takes care of the state machine
//! independent of the I/O and the second `Io` is responsible with I/O communication.  There are
//! two types to manage them: the [Io] and the [Sans], which are constructed by the [new] function.
//! These two parts communicate using `Request` and `Respond` types, which are defined by the user
//! (for real scenarios they could be `enums`).
//!
//! `Sans` starts communicating with `Io` using [Sans::start] and providing the initial `Request`;
//! it returns the [SansResponse] from the `Io`.  `Io` starts sans task by using [Io::start] which
//! returns [IoRequest] from `Sans`. The later communication is done using [Sans::handle] and
//! [Io::handle], which consume [SansResponse] and [IoRequest].
//!
//! See also more [examples](https://github.com/ewienik/asansio/tree/master/examples).
//!
//! ## Safety
//!
//! The crate uses `unsafe` parts for preparing a proper `async/await` infrastructure. Safety is
//! guaranteed by consuming the latest [IoRequest] and [SansResponse] - these handlers store
//! `Request` and `Response` objects and their lifetime is limited to the adjecent calls.

#![no_std]

use core::marker::PhantomData;
use core::pin::Pin;
use core::ptr;
use core::task::Context;
use core::task::Poll;
use core::task::RawWaker;
use core::task::RawWakerVTable;
use core::task::Waker;

/// Store transmission message from(Tx) or to(Rx) Sans
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

/// The Future helper for handling data between Io and Sans
pub struct SansHandle<'a, Request, Response> {
    request: Option<&'a Request>,
    _response: PhantomData<Response>,
}

impl<'a, Request: Unpin, Response: Unpin> Future for SansHandle<'a, Request, Response> {
    type Output = SansResponse<Response>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let waker = cx.waker();
        assert!(ptr::eq(waker.vtable(), &WAKER_VTABLE));

        // It is safe as waker is build befor each future handle call and the Channel
        // is valid between await points.
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

/// Manages the Sans part
pub struct Sans<Request, Response> {
    _request: PhantomData<Request>,
    _response: PhantomData<Response>,
}

/// The holder of the Response from the Io to Sans
pub struct SansResponse<Response> {
    response: *const Response,
}

// It is safe as its lifetime is between two awaits in the Sans part
unsafe impl<Response> Send for SansResponse<Response> {}

impl<Request, Response> Sans<Request, Response> {
    /// Initial request from the Sans part.
    pub fn start<'a>(&self, request: &'a Request) -> SansHandle<'a, Request, Response> {
        SansHandle {
            request: Some(request),
            _response: PhantomData,
        }
    }

    /// Next requests from the Sans part. It must receive SansResponse from the previous await call
    /// as the Response is not longer valid.
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
    /// Retrieve a reference to the Response from the Io part.
    pub fn response(&self) -> Option<&Response> {
        if self.response.is_null() {
            return None;
        }

        // It is save as SansResponse is used only between two adjacent await points
        Some(unsafe { &*self.response })
    }
}

/// Manages the Io part
pub struct Io<Request, Response> {
    _request: PhantomData<Request>,
    _response: PhantomData<Response>,
}

/// The holder of the Request from the Sans to Io
pub struct IoRequest<'a, Request, Task> {
    request: Option<&'a Request>,
    task: Pin<&'a mut Task>,
}

impl<Request, Response> Io<Request, Response> {
    /// Starts the Sans part defined as a Future Task. Returns on the first async Request from Sans
    /// or when the Task finishes.
    pub fn start<'a, Task>(&self, task: Pin<&'a mut Task>) -> Option<IoRequest<'a, Request, Task>>
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

    /// Next polling of the Future Task of the Sans part. It must receive IoRequest from the
    /// previous await call as the Response is not longer valid. Returns on the Request from Sans
    /// or when the Task finishes.
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
    /// Retrieve a reference to the Request from the Sans part.
    pub fn request(&self) -> Option<&Request> {
        self.request
    }

    fn run_async<Response>(&mut self, ch: Channel<Request, Response>) {
        // It is safe as now there is no valid Request waiting (IoRequest was consumed)
        let waker = unsafe { Waker::new(&ch as *const _ as *const (), &WAKER_VTABLE) };

        let mut cx = Context::from_waker(&waker);
        self.request = match self.task.as_mut().poll(&mut cx) {
            Poll::Ready(_) => None,
            Poll::Pending => {
                let Channel::Tx(request) = ch else {
                    unreachable!();
                };

                // It is safe as this will be the only one IoRequest and it will be consumed by the
                // next handle call
                Some(unsafe { &*request })
            }
        }
    }
}

/// Creates a two parts: Sans and Io for the specified Request and Response.
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

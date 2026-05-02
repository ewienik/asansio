#[derive(Debug)]
pub struct Message {
    pub tag: u8,
    pub buf: Box<[u8]>,
}

impl Message {
    pub fn new(tag: u8, buf: Box<[u8]>) -> Option<Self> {
        (buf.len() <= u8::MAX as usize).then_some(Self { tag, buf })
    }
}

#[derive(Debug)]
pub enum Recv {
    Downstream(usize),
    Upstream(Message),
}

pub trait Iface {
    type Error;

    async fn allocate(&mut self, size: usize) -> Result<Box<[u8]>, Self::Error>;
    async fn recv(&mut self, downstream_buf: &mut [u8]) -> Result<Recv, Self::Error>;
    async fn send_downstream(&mut self, buf: &[u8]) -> Result<usize, Self::Error>;
    async fn send_upstream(&mut self, message: Message) -> Result<(), Self::Error>;
}

struct Cache {
    recv: Box<[u8]>,
    recv_len: usize,
    send: Box<[u8]>,
    send_len: usize,
    send_consumed: usize,
}

impl Cache {
    const HEADER_SIZE: usize = 2;
    const MAX_PACKET_SIZE: usize = u8::MAX as usize + Self::HEADER_SIZE;

    async fn new(callbacks: &mut impl Iface) -> Result<Self, Error> {
        let recv = callbacks
            .allocate(Self::MAX_PACKET_SIZE)
            .await
            .map_err(|_| Error::Allocate)?;
        let send = callbacks
            .allocate(Self::MAX_PACKET_SIZE)
            .await
            .map_err(|_| Error::Allocate)?;
        Ok(Self {
            recv,
            recv_len: 0,
            send,
            send_len: 0,
            send_consumed: 0,
        })
    }

    fn recv_downstream_buffer(&mut self) -> &mut [u8] {
        if self.recv_len < Self::HEADER_SIZE {
            &mut self.recv[self.recv_len..Self::HEADER_SIZE]
        } else {
            let len = self.recv[1] as usize + Self::HEADER_SIZE;
            &mut self.recv[self.recv_len..len]
        }
    }

    fn recv_downstream_consumed(&mut self, len: usize) -> Result<(), Error> {
        if self.recv.len() < self.recv_len + len {
            return Err(Error::RecvDownstreamConsumed);
        }
        self.recv_len += len;
        Ok(())
    }

    fn send_downstream_buffer(&self) -> Option<&[u8]> {
        (self.send_len > self.send_consumed)
            .then_some(&self.send[self.send_consumed..self.send_len])
    }

    fn send_downstream_consumed(&mut self, len: usize) -> Result<(), Error> {
        if self.send_len < self.send_consumed + len {
            return Err(Error::SendDownstreamConsumed);
        }
        self.send_consumed += len;
        if self.send_consumed == self.send_len {
            self.send_len = 0;
            self.send_consumed = 0;
        }
        Ok(())
    }

    fn recv_upstream(&mut self, message: Message) -> Result<(), Error> {
        if self.send_len > 0 {
            return Err(Error::Internal);
        }
        self.send_len = message.buf.len() + Self::HEADER_SIZE;
        self.send[0] = message.tag;
        self.send[1] = message.buf.len() as u8;
        self.send[2..self.send_len].copy_from_slice(&message.buf);
        Ok(())
    }

    async fn send_upstream(
        &mut self,
        callbacks: &mut impl Iface,
    ) -> Result<Option<Message>, Error> {
        if self.recv_len < Self::HEADER_SIZE {
            return Ok(None);
        }
        let len = self.recv[1] as usize + Self::HEADER_SIZE;
        if self.recv_len < len {
            return Ok(None);
        }
        let mut buf = callbacks
            .allocate(len - Self::HEADER_SIZE)
            .await
            .map_err(|_| Error::Allocate)?;
        buf.copy_from_slice(&self.recv[Self::HEADER_SIZE..len]);
        self.recv_len = 0;
        Ok(Some(Message {
            tag: self.recv[0],
            buf,
        }))
    }
}

#[derive(Clone, Debug)]
pub enum Error {
    Allocate,
    Recv,
    RecvDownstreamConsumed,
    SendDownstreamConsumed,
    SendUpstream,
    SendDownstream,
    Internal,
}

pub async fn run(mut callbacks: impl Iface) -> Result<(), Error> {
    let mut cache = Cache::new(&mut callbacks).await?;

    loop {
        let recv = callbacks
            .recv(cache.recv_downstream_buffer())
            .await
            .map_err(|_| Error::Recv)?;
        match recv {
            Recv::Downstream(len) => cache.recv_downstream_consumed(len)?,
            Recv::Upstream(message) => cache.recv_upstream(message)?,
        }

        if let Some(message) = cache.send_upstream(&mut callbacks).await? {
            callbacks
                .send_upstream(message)
                .await
                .map_err(|_| Error::SendUpstream)?;
        }

        while let Some(buf) = cache.send_downstream_buffer() {
            let len = callbacks
                .send_downstream(buf)
                .await
                .map_err(|_| Error::SendDownstream)?;
            cache.send_downstream_consumed(len)?;
        }
    }
}

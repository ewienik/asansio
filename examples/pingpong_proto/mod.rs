use std::time::Duration;

#[derive(Debug)]
pub enum Message {
    Message(String),
    Sleep(Duration),
}

impl Message {
    pub fn new_message(msg: impl Into<String> + AsRef<str>) -> Option<Self> {
        (msg.as_ref().len() <= u8::MAX as usize).then_some(Self::Message(msg.into()))
    }
}

#[derive(Debug)]
pub enum Payload {
    Message(Box<[u8]>),
    Sleep(Box<[u8]>),
}

impl TryFrom<Payload> for Message {
    type Error = Error;

    fn try_from(payload: Payload) -> Result<Self, Self::Error> {
        Ok(match payload {
            Payload::Message(buf) => {
                Message::Message(String::from_utf8(buf.into()).map_err(|_| Error::PayloadFormat)?)
            }
            Payload::Sleep(buf) => Message::Sleep(Duration::from_millis(
                buf.first().copied().ok_or(Error::PayloadFormat)? as u64,
            )),
        })
    }
}

#[derive(Debug)]
pub enum Recv {
    Downstream(Payload),
    Upstream(Message),
}

pub trait Iface {
    type Error;

    async fn allocate(&mut self, size: usize) -> Result<Box<[u8]>, Self::Error>;
    async fn recv(&mut self) -> Result<Recv, Self::Error>;
    async fn send_downstream(&mut self, payload: Payload) -> Result<(), Self::Error>;
    async fn send_upstream(&mut self, message: Message) -> Result<(), Self::Error>;
}

#[derive(Clone, Debug)]
pub enum Error {
    Allocate,
    Recv,
    PayloadFormat,
    SendUpstream,
    SendDownstream,
}

async fn to_payload(callbacks: &mut impl Iface, message: Message) -> Result<Payload, Error> {
    Ok(match message {
        Message::Message(message) => {
            let bytes = message.as_bytes();
            let mut buf = callbacks
                .allocate(bytes.len())
                .await
                .map_err(|_| Error::Allocate)?;
            buf.copy_from_slice(bytes);
            Payload::Message(buf)
        }
        Message::Sleep(duration) => {
            let mut buf = callbacks.allocate(1).await.map_err(|_| Error::Allocate)?;
            buf[0] = duration.as_millis().clamp(0, u8::MAX as u128) as u8;
            Payload::Sleep(buf)
        }
    })
}

pub async fn run(mut callbacks: impl Iface) -> Result<(), Error> {
    loop {
        let recv = callbacks.recv().await.map_err(|_| Error::Recv)?;

        match recv {
            Recv::Downstream(payload) => callbacks
                .send_upstream(payload.try_into()?)
                .await
                .map_err(|_| Error::SendUpstream)?,
            Recv::Upstream(message) => {
                let payload = to_payload(&mut callbacks, message).await?;
                callbacks
                    .send_downstream(payload)
                    .await
                    .map_err(|_| Error::SendDownstream)?
            }
        }
    }
}

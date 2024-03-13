use std::{io, task::Poll};
use std::pin::Pin;
use std::task::Context;

use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};
use tokio::io::ReadBuf;

use crate::{error::Result, Message};

#[derive(Debug)]
/// Implements the `AsyncRead` and `AsyncWrite` traits.
///
/// Shutdown permanently closes the quic stream.
pub struct QuicStream {
    pub(crate) id: u64,
    pub(crate) rx: UnboundedReceiver<Result<Message>>,
    pub(crate) tx: UnboundedSender<Message>,
}

impl QuicStream {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn split(self) -> (crate::stream::QuicStreamReader, crate::stream::QuicStreamWriter) {
        let reader = crate::stream::QuicStreamReader { id: self.id, rx: self.rx };
        let writer = crate::stream::QuicStreamWriter { id: self.id, tx: self.tx };
        (reader, writer)
    }
}


pub struct QuicStreamReader {
    id: u64,
    rx: UnboundedReceiver<Result<Message>>,
}


impl AsyncRead for QuicStreamReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let self_mut = self.get_mut();
        match self_mut.rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(message))) => match message {
                Message::Data { bytes, .. } => {
                    buf.put_slice(&bytes);
                    Poll::Ready(Ok(()))
                },
                _ => Poll::Ready(Ok(())), // Handle other message types if necessary
            },
            Poll::Ready(Some(Err(e))) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e.to_string()))),
            Poll::Ready(None) => Poll::Ready(Ok(())), // Stream ended
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct QuicStreamWriter {
    id: u64,
    tx: UnboundedSender<Message>,
}

impl AsyncWrite for QuicStreamWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let message = Message::Data {
            stream_id: self.id, // Directly access `id` here
            bytes: buf.to_vec(),
            fin: false,
        };
        match self.tx.send(message) { // Directly access `tx` here
            Ok(_) => Poll::Ready(Ok(buf.len())),
            Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, err.to_string()))),
        }
    }


    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Implement if needed
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for QuicStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(message)) => match message {
                Ok(Message::Data {
                       stream_id: _,
                       bytes,
                       fin: _,
                   }) => {
                    buf.put_slice(bytes.as_slice());
                    buf.set_filled(bytes.len());
                    Poll::Ready(Ok(()))
                }
                Ok(Message::Close(_id)) => Poll::Ready(Ok(())),
                Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, err.to_string()))),
            },
            Poll::Ready(None) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "No new data is available to be read!",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for QuicStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, io::Error>> {
        let message = Message::Data {
            stream_id: self.id,
            bytes: buf.to_vec(),
            fin: false,
        };
        match self.tx.send(message) {
            Ok(_) => Poll::Ready(Ok(buf.len())),
            Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, err))),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), io::Error>> {
        let message = Message::Close(self.id);
        match self.tx.send(message) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, err))),
        }
    }
}

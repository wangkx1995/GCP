use crate::message::InternalMessage;
use anyhow::Result;
use tokio_util::bytes::Bytes;
use tokio_util::codec::LengthDelimitedCodec;
use tokio_util::codec::FramedRead;
use tokio_util::codec::FramedWrite;
use futures::{SinkExt, StreamExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use uuid::Uuid;

const MAGIC: &[u8; 4] = b"GCPM";

pub fn new_framed_write(writer: OwnedWriteHalf) -> FramedWrite<OwnedWriteHalf, LengthDelimitedCodec> {
    FramedWrite::new(writer, LengthDelimitedCodec::new())
}

pub fn new_framed_read(reader: OwnedReadHalf) -> FramedRead<OwnedReadHalf, LengthDelimitedCodec> {
    FramedRead::new(reader, LengthDelimitedCodec::new())
}

pub async fn send_message(
    tx: &mut FramedWrite<OwnedWriteHalf, LengthDelimitedCodec>,
    msg: &InternalMessage,
) -> Result<()> {
    let payload = bincode::serialize(msg)?;
    let mut buf = Vec::with_capacity(20 + payload.len());
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&Uuid::new_v4().into_bytes());
    buf.extend_from_slice(&payload);
    tx.send(Bytes::from(buf)).await?;
    Ok(())
}

pub async fn recv_message(
    rx: &mut FramedRead<OwnedReadHalf, LengthDelimitedCodec>,
) -> Result<Option<InternalMessage>> {
    match rx.next().await {
        Some(Ok(buf)) => {
            if buf.len() < 20 {
                anyhow::bail!("frame too short: {} bytes", buf.len());
            }
            let payload = &buf[20..];
            let msg = bincode::deserialize(payload)?;
            Ok(Some(msg))
        }
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn test_send_recv_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let mut framed_rx = new_framed_read(reader);
            let mut framed_tx = new_framed_write(writer);

            let msg = recv_message(&mut framed_rx).await.unwrap().unwrap();
            assert!(matches!(msg, InternalMessage::HeartbeatAck));
            send_message(&mut framed_tx, &InternalMessage::Shutdown).await.unwrap();
        });

        let stream = TcpStream::connect(addr).await.unwrap();
        let (reader, writer) = stream.into_split();
        let mut framed_rx = new_framed_read(reader);
        let mut framed_tx = new_framed_write(writer);

        send_message(&mut framed_tx, &InternalMessage::HeartbeatAck).await.unwrap();
        let msg = recv_message(&mut framed_rx).await.unwrap().unwrap();
        assert!(matches!(msg, InternalMessage::Shutdown));

        server.await.unwrap();
    }
}

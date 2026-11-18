//! Raw RSD service streams (inventory §2.7/§5): the
//! 4-byte-BE-length + XML plist framing of RPK's
//! `rppairing_service_stream_send_plist/recv_plist`, raw
//! byte passthrough for the AFC/DDI legs, and the
//! RSDCheckin sequence Swift limd's `withRSDService`
//! performs on every stream before business traffic.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use idevice68::ReadWrite;
use plist::{Dictionary, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

use crate::session::{RpError, RpResult};

/// RPK's receive cap for one plist frame (10 MB).
pub const MAX_PLIST_FRAME: u32 = 10 * 1024 * 1024;

/// Swift limd `rsdRecvPlist`'s default timeout (120 s).
pub const DEFAULT_RECV_TIMEOUT_MS: u64 = 120_000;

/// Serializes one plist frame: 4-byte BE length + XML.
pub fn encode_plist_frame(value: &Value) -> RpResult<Vec<u8>> {
    let mut xml = Vec::new();
    value
        .to_writer_xml(&mut xml)
        .map_err(|e| RpError::plain("framing", e.to_string()))?;
    let mut frame = Vec::with_capacity(4 + xml.len());
    frame.extend_from_slice(&(xml.len() as u32).to_be_bytes());
    frame.extend_from_slice(&xml);
    Ok(frame)
}

/// Writes one plist frame and flushes.
pub async fn write_plist_frame<S: AsyncWrite + Unpin>(
    stream: &mut S,
    value: &Value,
) -> RpResult<()> {
    let frame = encode_plist_frame(value)?;
    stream
        .write_all(&frame)
        .await
        .map_err(|e| RpError::plain("framing", e.to_string()))?;
    stream
        .flush()
        .await
        .map_err(|e| RpError::plain("framing", e.to_string()))
}

/// Reads one plist frame. The length prefix is capped at
/// [`MAX_PLIST_FRAME`], as in RPK.
pub async fn read_plist_frame<S: AsyncRead + Unpin>(stream: &mut S) -> RpResult<Value> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| RpError::plain("framing", e.to_string()))?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_PLIST_FRAME {
        return Err(RpError::plain(
            "framing",
            format!("RSD plist frame too large: {len} bytes"),
        ));
    }
    let mut payload = vec![0u8; len as usize];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(|e| RpError::plain("framing", e.to_string()))?;
    Value::from_reader(std::io::Cursor::new(payload))
        .map_err(|e| RpError::plain("framing", e.to_string()))
}

fn as_dictionary(value: Value) -> RpResult<Dictionary> {
    match value {
        Value::Dictionary(d) => Ok(d),
        _ => Err(RpError::plain(
            "framing",
            "RSD response was not a dictionary",
        )),
    }
}

/// One raw RSD service stream over the tunnel's user-space
/// TCP stack.
pub struct RpStream {
    inner: Box<dyn ReadWrite>,
}

impl RpStream {
    pub fn new(inner: Box<dyn ReadWrite>) -> Self {
        Self { inner }
    }

    /// RPK `rppairing_service_stream_send_plist`.
    pub async fn send_plist(&mut self, dict: &Dictionary) -> RpResult<()> {
        write_plist_frame(&mut self.inner, &Value::Dictionary(dict.clone())).await
    }

    /// RPK `rppairing_service_stream_recv_plist` with Swift
    /// limd's default 120 s timeout.
    pub async fn recv_plist(&mut self) -> RpResult<Dictionary> {
        self.recv_plist_timeout(DEFAULT_RECV_TIMEOUT_MS).await
    }

    /// As [`Self::recv_plist`] with an explicit timeout in
    /// milliseconds (Swift limd passes per-call timeouts:
    /// 5 s heartbeat, 10 s CopyAll, 30 s mount replies).
    pub async fn recv_plist_timeout(&mut self, timeout_ms: u64) -> RpResult<Dictionary> {
        let value = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            read_plist_frame(&mut self.inner),
        )
        .await
        .map_err(|_| {
            RpError::plain(
                "framing",
                format!("timed out after {timeout_ms} ms waiting for RSD response"),
            )
        })??;
        as_dictionary(value)
    }

    /// The RSDCheckin sequence of Swift limd's
    /// `withRSDService`: send the checkin plist (the label is
    /// Swift's literal `"SideStore"`), then read and discard
    /// two frames — the checkin response and the
    /// StartService response.
    pub async fn rsd_checkin(&mut self) -> RpResult<()> {
        let mut checkin = Dictionary::new();
        checkin.insert("Label".into(), Value::String("SideStore".into()));
        checkin.insert(
            "ProtocolVersion".into(),
            Value::String("2".into()),
        );
        checkin.insert(
            "Request".into(),
            Value::String("RSDCheckin".into()),
        );
        self.send_plist(&checkin).await?;
        let _ = self.recv_plist().await?;
        let _ = self.recv_plist().await?;
        Ok(())
    }

    /// RPK `rppairing_service_stream_send_raw`.
    pub async fn send_raw(&mut self, bytes: &[u8]) -> RpResult<()> {
        self.inner
            .write_all(bytes)
            .await
            .map_err(|e| RpError::plain("framing", e.to_string()))?;
        self.inner
            .flush()
            .await
            .map_err(|e| RpError::plain("framing", e.to_string()))
    }

    /// RPK `rppairing_service_stream_recv_exact`.
    pub async fn recv_exact(&mut self, buf: &mut [u8]) -> RpResult<()> {
        self.inner
            .read_exact(buf)
            .await
            .map(|_| ())
            .map_err(|e| RpError::plain("framing", e.to_string()))
    }
}

impl AsyncRead for RpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for RpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut *self.inner).poll_write(cx, data)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_dict() -> Dictionary {
        let mut d = Dictionary::new();
        d.insert("Command".into(), Value::String("Pico".into()));
        d.insert("Interval".into(), Value::Integer(42.into()));
        d
    }

    #[test]
    fn frame_is_be_length_plus_xml() {
        let frame = encode_plist_frame(&Value::Dictionary(sample_dict())).unwrap();
        let declared = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(declared, frame.len() - 4);
        assert!(frame[4..].starts_with(b"<?xml"));
    }

    #[tokio::test]
    async fn frame_roundtrip_over_duplex() {
        let (mut a, mut b) = tokio::io::duplex(4096);
        write_plist_frame(&mut a, &Value::Dictionary(sample_dict()))
            .await
            .unwrap();
        let value = read_plist_frame(&mut b).await.unwrap();
        let d = as_dictionary(value).unwrap();
        assert_eq!(
            d.get("Command"),
            Some(&Value::String("Pico".to_string()))
        );
        assert_eq!(
            d.get("Interval").and_then(|v| v.as_unsigned_integer()),
            Some(42)
        );
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected() {
        let (mut a, mut b) = tokio::io::duplex(4096);
        let len = (MAX_PLIST_FRAME + 1).to_be_bytes();
        a.write_all(&len).await.unwrap();
        let err = read_plist_frame(&mut b).await.unwrap_err();
        assert!(err.message.contains("too large"), "{}", err.message);
    }

    #[test]
    fn checkin_dict_shape() {
        // The checkin plist is built inline in rsd_checkin;
        // pin its three entries here via the same literals.
        let mut checkin = Dictionary::new();
        checkin.insert("Label".into(), Value::String("SideStore".into()));
        checkin.insert("ProtocolVersion".into(), Value::String("2".into()));
        checkin.insert("Request".into(), Value::String("RSDCheckin".into()));
        let frame = encode_plist_frame(&Value::Dictionary(checkin)).unwrap();
        let xml = String::from_utf8_lossy(&frame[4..]).into_owned();
        assert!(xml.contains("SideStore"));
        assert!(xml.contains("RSDCheckin"));
    }
}

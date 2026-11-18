//! RSD AFC packet codec — port of the `rsdAfc*` helpers in
//! Swift `LibimobiledeviceGateway` (yongshuai1013/minimuxer
//! @714aafa3), used by the RemotePairing (RSD) service streams.
//!
//! M5 lands the codec and the stream operation layer with
//! fixtures (DESIGN §4 M5 acceptance); wiring it into the two
//! backends' RP paths is M7. Behaviour is catalogued in
//! `docs/m5-install-ddi-debug-diff.md` §6.

use mm_common::{DeviceGatewayCode, MmErrorInfo};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::gateway::gw_error;

/// "CFA6LPAA" little-endian.
pub const AFC_MAGIC: u64 = 0x4141504C36414643;
/// Swift `kAfcChunkSize` — 1 MB bulk-transfer chunk.
pub const AFC_CHUNK_SIZE: usize = 1024 * 1024;

pub const OP_STATUS: u64 = 1;
pub const OP_MAKE_DIR: u64 = 9;
pub const OP_FILE_OPEN: u64 = 13;
pub const OP_WRITE: u64 = 16;
pub const OP_FILE_CLOSE: u64 = 20;

const HEADER_LEN: usize = 40;

/// One decoded AFC packet (Swift `rsdAfcRecvResponse`'s tuple).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsdAfcPacket {
    pub opcode: u64,
    pub header_payload: Vec<u8>,
    pub payload: Vec<u8>,
}

/// Encodes one packet (Swift `rsdAfcSendPacket`): the 40-byte
/// little-endian header (magic, entire_len, header_payload_len,
/// packet_num, opcode), then header payload, then payload.
pub fn encode_packet(
    opcode: u64,
    header_payload: &[u8],
    payload: &[u8],
    packet_num: u64,
) -> Vec<u8> {
    let entire_len = (HEADER_LEN + header_payload.len() + payload.len()) as u64;
    let header_payload_len = (HEADER_LEN + header_payload.len()) as u64;
    let mut out = Vec::with_capacity(entire_len as usize);
    out.extend_from_slice(&AFC_MAGIC.to_le_bytes());
    out.extend_from_slice(&entire_len.to_le_bytes());
    out.extend_from_slice(&header_payload_len.to_le_bytes());
    out.extend_from_slice(&packet_num.to_le_bytes());
    out.extend_from_slice(&opcode.to_le_bytes());
    out.extend_from_slice(header_payload);
    out.extend_from_slice(payload);
    out
}

fn le_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

/// Decodes one packet from its full wire bytes (header +
/// payloads); `None` when the buffer is shorter than the
/// header declares.
pub fn decode_packet(bytes: &[u8]) -> Option<RsdAfcPacket> {
    if bytes.len() < HEADER_LEN {
        return None;
    }
    if le_u64(bytes, 0) != AFC_MAGIC {
        return None;
    }
    let entire_len = le_u64(bytes, 8) as usize;
    let header_payload_len = le_u64(bytes, 16) as usize;
    let opcode = le_u64(bytes, 32);
    if entire_len > bytes.len() || header_payload_len < HEADER_LEN || entire_len < header_payload_len
    {
        return None;
    }
    let header_payload = bytes[HEADER_LEN..header_payload_len].to_vec();
    let payload = bytes[header_payload_len..entire_len].to_vec();
    Some(RsdAfcPacket {
        opcode,
        header_payload,
        payload,
    })
}

/// Sends one packet and advances the caller's packet counter
/// (Swift `rsdAfcSendPacket` — the counter increments per send,
/// and each Swift operation starts it at 0).
pub async fn send_packet<S>(
    stream: &mut S,
    opcode: u64,
    header_payload: &[u8],
    payload: &[u8],
    packet_num: &mut u64,
) -> Result<(), MmErrorInfo>
where
    S: AsyncWrite + Unpin,
{
    let bytes = encode_packet(opcode, header_payload, payload, *packet_num);
    *packet_num += 1;
    stream.write_all(&bytes).await.map_err(|e| {
        gw_error(
            DeviceGatewayCode::ServiceError,
            format!("AFC send packet failed: {e}"),
        )
    })?;
    stream.flush().await.map_err(|e| {
        gw_error(
            DeviceGatewayCode::ServiceError,
            format!("AFC send packet failed: {e}"),
        )
    })?;
    Ok(())
}

/// Receives one packet (Swift `rsdAfcRecvResponse`).
pub async fn recv_response<S>(stream: &mut S) -> Result<RsdAfcPacket, MmErrorInfo>
where
    S: AsyncRead + Unpin,
{
    let mut header = [0u8; HEADER_LEN];
    stream.read_exact(&mut header).await.map_err(|e| {
        gw_error(
            DeviceGatewayCode::ServiceError,
            format!("AFC recv header failed: {e}"),
        )
    })?;
    let entire_len = le_u64(&header, 8) as usize;
    let header_payload_len = le_u64(&header, 16) as usize;
    let opcode = le_u64(&header, 32);
    let header_payload_size = header_payload_len.saturating_sub(HEADER_LEN);
    let payload_size = entire_len.saturating_sub(header_payload_len);

    let mut header_payload = vec![0u8; header_payload_size];
    stream
        .read_exact(&mut header_payload)
        .await
        .map_err(|e| {
            gw_error(
                DeviceGatewayCode::ServiceError,
                format!("AFC recv header payload failed: {e}"),
            )
        })?;
    let mut payload = vec![0u8; payload_size];
    stream.read_exact(&mut payload).await.map_err(|e| {
        gw_error(
            DeviceGatewayCode::ServiceError,
            format!("AFC recv payload failed: {e}"),
        )
    })?;
    Ok(RsdAfcPacket {
        opcode,
        header_payload,
        payload,
    })
}

/// Swift `rsdAfcMakeDir`: the path goes in the header payload
/// with a trailing NUL; the response is read and discarded
/// (errors swallowed by the caller-side `try?` there — here the
/// receive error is swallowed too, the send error is not).
pub async fn make_dir<S>(stream: &mut S, path: &str, packet_num: &mut u64) -> Result<(), MmErrorInfo>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut payload = path.as_bytes().to_vec();
    payload.push(0);
    send_packet(stream, OP_MAKE_DIR, &payload, &[], packet_num).await?;
    let _ = recv_response(stream).await;
    Ok(())
}

/// Swift `rsdAfcFileOpen`: header payload is the mode (LE u64)
/// plus the NUL-terminated path; the response's header payload
/// carries the handle.
pub async fn file_open<S>(
    stream: &mut S,
    path: &str,
    mode: u64,
    packet_num: &mut u64,
) -> Result<u64, MmErrorInfo>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut open_payload = mode.to_le_bytes().to_vec();
    open_payload.extend_from_slice(path.as_bytes());
    open_payload.push(0);
    send_packet(stream, OP_FILE_OPEN, &open_payload, &[], packet_num).await?;
    let resp = recv_response(stream).await?;
    if resp.header_payload.len() < 8 {
        return Err(gw_error(
            DeviceGatewayCode::ServiceError,
            format!("AFC FileOpen response invalid for {path}"),
        ));
    }
    let fd = le_u64(&resp.header_payload, 0);
    if fd == 0 {
        return Err(gw_error(
            DeviceGatewayCode::ServiceError,
            format!("AFC FileOpen returned fd 0 for {path}"),
        ));
    }
    Ok(fd)
}

/// Swift `rsdAfcFileClose`: response read and discarded.
pub async fn file_close<S>(stream: &mut S, fd: u64, packet_num: &mut u64) -> Result<(), MmErrorInfo>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    send_packet(stream, OP_FILE_CLOSE, &fd.to_le_bytes(), &[], packet_num).await?;
    let _ = recv_response(stream).await;
    Ok(())
}

/// Writes `data` to an open handle in `AFC_CHUNK_SIZE` chunks
/// (the Swift RSD send loops of `syncsendIpaAfc` /
/// `syncsendAppBundleAfc`): each chunk's Status response is
/// checked — opcode 1 with a non-zero status is an error.
pub async fn write_data<S>(
    stream: &mut S,
    fd: u64,
    data: &[u8],
    path_for_errors: Option<&str>,
    packet_num: &mut u64,
) -> Result<(), MmErrorInfo>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + AFC_CHUNK_SIZE).min(data.len());
        let chunk = &data[offset..end];
        send_packet(stream, OP_WRITE, &fd.to_le_bytes(), chunk, packet_num).await?;
        let resp = recv_response(stream).await?;
        if resp.opcode == OP_STATUS && resp.header_payload.len() >= 8 {
            let status = le_u64(&resp.header_payload, 0);
            if status != 0 {
                return Err(gw_error(
                    DeviceGatewayCode::ServiceError,
                    match path_for_errors {
                        Some(path) => {
                            format!("AFC Write failed with status {status} for {path}")
                        }
                        None => format!("AFC Write failed with status {status}"),
                    },
                ));
            }
        }
        offset = end;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn encode_layout_matches_swift_header() {
        // MakeDir for "PublicStaging": path + NUL in the header
        // payload, packet number 0.
        let mut payload = b"PublicStaging".to_vec();
        payload.push(0);
        let bytes = encode_packet(OP_MAKE_DIR, &payload, &[], 0);
        assert_eq!(bytes.len(), 40 + 14);
        assert_eq!(le_u64(&bytes, 0), 0x4141504C36414643);
        // "CFA6LPAA" as stored little-endian.
        assert_eq!(&bytes[0..8], b"CFA6LPAA");
        assert_eq!(le_u64(&bytes, 8), 54); // entire_len
        assert_eq!(le_u64(&bytes, 16), 54); // header_payload_len
        assert_eq!(le_u64(&bytes, 24), 0); // packet_num
        assert_eq!(le_u64(&bytes, 32), 9); // MakeDir
        assert_eq!(&bytes[40..], b"PublicStaging\0");
    }

    #[test]
    fn encode_file_open_and_write_shapes() {
        let mut open = 3u64.to_le_bytes().to_vec();
        open.extend_from_slice(b"PublicStaging/x.ipa");
        open.push(0);
        let bytes = encode_packet(OP_FILE_OPEN, &open, &[], 7);
        assert_eq!(le_u64(&bytes, 24), 7);
        assert_eq!(le_u64(&bytes, 32), 13);
        assert_eq!(le_u64(&bytes[40..48], 0), 3);

        let bytes = encode_packet(OP_WRITE, &42u64.to_le_bytes(), b"DATA", 8);
        assert_eq!(le_u64(&bytes, 8), 40 + 8 + 4);
        assert_eq!(le_u64(&bytes, 16), 40 + 8);
        assert_eq!(le_u64(&bytes, 32), 16);
        assert_eq!(&bytes[48..], b"DATA");
    }

    #[test]
    fn decode_round_trips_and_rejects_bad_magic() {
        let bytes = encode_packet(OP_STATUS, &0u64.to_le_bytes(), &[], 3);
        let packet = decode_packet(&bytes).unwrap();
        assert_eq!(packet.opcode, OP_STATUS);
        assert_eq!(packet.header_payload, 0u64.to_le_bytes().to_vec());
        assert!(packet.payload.is_empty());

        let mut bad = bytes.clone();
        bad[0] = 0;
        assert!(decode_packet(&bad).is_none());
        assert!(decode_packet(&bytes[..20]).is_none());
    }

    /// A scripted in-memory AFC peer: answers MakeDir with a
    /// Status 0, FileOpen with handle 9, Write with Status 0,
    /// FileClose with Status 0 — recording every request packet.
    async fn run_scripted_peer(
        mut stream: tokio::io::DuplexStream,
        record: std::sync::Arc<std::sync::Mutex<Vec<RsdAfcPacket>>>,
    ) {
        let mut count = 0u64;
        loop {
            let packet = match recv_response(&mut stream).await {
                Ok(p) => p,
                Err(_) => return,
            };
            let opcode = packet.opcode;
            record.lock().unwrap().push(packet);
            let (resp_opcode, resp_hp): (u64, Vec<u8>) = match opcode {
                OP_FILE_OPEN => (0x0E, 9u64.to_le_bytes().to_vec()),
                _ => (OP_STATUS, 0u64.to_le_bytes().to_vec()),
            };
            let bytes = encode_packet(resp_opcode, &resp_hp, &[], count);
            count += 1;
            use tokio::io::AsyncWriteExt as _;
            if stream.write_all(&bytes).await.is_err() {
                return;
            }
        }
    }

    #[tokio::test]
    async fn scripted_send_ipa_sequence() {
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let record = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let peer = tokio::spawn(run_scripted_peer(server, record.clone()));

        // The RSD sendIpaAfc sequence (packet numbers restart
        // at 0 per Swift operation).
        let mut num = 0u64;
        make_dir(&mut client, "PublicStaging", &mut num).await.unwrap();
        let fd = file_open(&mut client, "PublicStaging/x.ipa", 3, &mut num)
            .await
            .unwrap();
        assert_eq!(fd, 9);
        let data = vec![7u8; 100];
        write_data(&mut client, fd, &data, None, &mut num).await.unwrap();
        file_close(&mut client, fd, &mut num).await.unwrap();
        drop(client);
        peer.abort();

        let requests = record.lock().unwrap();
        let opcodes: Vec<u64> = requests.iter().map(|p| p.opcode).collect();
        assert_eq!(opcodes, vec![OP_MAKE_DIR, OP_FILE_OPEN, OP_WRITE, OP_FILE_CLOSE]);
        assert_eq!(requests[2].payload.len(), 100);
        assert_eq!(le_u64(&requests[2].header_payload, 0), 9);
    }

    #[tokio::test]
    async fn write_status_error_surfaces() {
        // A peer that answers the Write with Status 8
        // (ObjectNotFound).
        let (mut client, mut server) = tokio::io::duplex(64 * 1024);
        tokio::spawn(async move {
            let _packet = recv_response(&mut server).await.unwrap();
            let bytes = encode_packet(OP_STATUS, &8u64.to_le_bytes(), &[], 0);
            use tokio::io::AsyncWriteExt as _;
            let _ = server.write_all(&bytes).await;
        });
        let mut num = 0u64;
        let err = write_data(&mut client, 9, b"payload", None, &mut num)
            .await
            .unwrap_err();
        assert_eq!(err.domain, "DeviceGatewayError");
        assert_eq!(err.code, "serviceError");
        assert_eq!(err.reason, "AFC Write failed with status 8");
    }

    #[tokio::test]
    async fn recv_response_reads_split_packets() {
        let bytes = encode_packet(OP_STATUS, &0u64.to_le_bytes(), b"tail", 0);
        let mut cursor = Cursor::new(bytes);
        let packet = recv_response(&mut cursor).await.unwrap();
        assert_eq!(packet.opcode, OP_STATUS);
        assert_eq!(packet.payload, b"tail");
    }
}

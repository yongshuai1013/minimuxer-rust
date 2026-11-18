//! `AfcSession` — a packet-level AFC session over the idevice
//! crate's `AfcClient`, for gw-idevice (diff list D-M5-3).
//!
//! idevice 0.1.29's `AfcClient` covers list/mkdir/info/remove
//! but has no file open/read/write, and an `Idevice`'s raw
//! methods are private to the crate. The client's public
//! `send`/`read` pair is the seam: this session builds every
//! packet itself (its own packet numbers in the packet
//! headers) and exchanges them through `AfcClient::send` /
//! `AfcClient::read` — whose Status decoding (a Status
//! response's header payload starts with the LE u64 `AfcError`
//! code) is exactly the check the operations need. The
//! client's built-in helpers are never called on a session
//! instance, so its private counter never advances and the
//! session's numbers are the only ones on the wire.

use idevice::afc::errors::AfcError;
use idevice::afc::opcode::{AfcFopenMode, AfcOpcode};
use idevice::afc::packet::{AfcPacket, AfcPacketHeader};
use idevice::afc::{AfcClient, MAGIC};
use idevice::IdeviceError;

/// Read chunk for whole-file reads (the AFC service answers a
/// Read with at most the requested length; a short answer ends
/// the file).
const READ_CHUNK: u64 = 1024 * 1024;

pub struct AfcSession {
    client: AfcClient,
    packet_num: u64,
}

impl AfcSession {
    pub fn new(client: AfcClient) -> Self {
        Self {
            client,
            packet_num: 0,
        }
    }

    fn packet(
        &mut self,
        operation: AfcOpcode,
        header_payload: Vec<u8>,
        payload: Vec<u8>,
    ) -> AfcPacket {
        let header_payload_len = AfcPacketHeader::LEN + header_payload.len() as u64;
        let entire_len = header_payload_len + payload.len() as u64;
        let packet = AfcPacket {
            header: AfcPacketHeader {
                magic: MAGIC,
                entire_len,
                header_payload_len,
                packet_num: self.packet_num,
                operation,
            },
            header_payload,
            payload,
        };
        self.packet_num += 1;
        packet
    }

    async fn exchange(
        &mut self,
        operation: AfcOpcode,
        header_payload: Vec<u8>,
        payload: Vec<u8>,
    ) -> Result<AfcPacket, IdeviceError> {
        let packet = self.packet(operation, header_payload, payload);
        self.client.send(packet).await?;
        self.client.read().await
    }

    pub async fn make_dir(&mut self, path: &str) -> Result<(), IdeviceError> {
        self.exchange(AfcOpcode::MakeDir, path.as_bytes().to_vec(), Vec::new())
            .await?;
        Ok(())
    }

    pub async fn list_dir(&mut self, path: &str) -> Result<Vec<String>, IdeviceError> {
        let res = self
            .exchange(AfcOpcode::ReadDir, path.as_bytes().to_vec(), Vec::new())
            .await?;
        Ok(res
            .payload
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect())
    }

    /// The raw key/value pairs of a GetFileInfo response (the
    /// gateway interprets `st_ifmt`/`st_size` per backend —
    /// gw-idevice's FFI struct semantics differ from the C
    /// library's, see the diff list §3/§4).
    pub async fn get_file_info(
        &mut self,
        path: &str,
    ) -> Result<Vec<(String, String)>, IdeviceError> {
        let res = self
            .exchange(
                AfcOpcode::GetFileInfo,
                path.as_bytes().to_vec(),
                Vec::new(),
            )
            .await?;
        let strings: Vec<String> = res
            .payload
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        Ok(strings
            .chunks_exact(2)
            .map(|chunk| (chunk[0].clone(), chunk[1].clone()))
            .collect())
    }

    pub async fn remove_path(&mut self, path: &str) -> Result<(), IdeviceError> {
        self.exchange(
            AfcOpcode::RemovePath,
            path.as_bytes().to_vec(),
            Vec::new(),
        )
        .await?;
        Ok(())
    }

    pub async fn file_open(
        &mut self,
        path: &str,
        mode: AfcFopenMode,
    ) -> Result<u64, IdeviceError> {
        let mut header_payload = (mode as u64).to_le_bytes().to_vec();
        header_payload.extend_from_slice(path.as_bytes());
        header_payload.push(0);
        let res = self
            .exchange(AfcOpcode::FileOpen, header_payload, Vec::new())
            .await?;
        if res.header.operation != AfcOpcode::FileOpenRes
            || res.header_payload.len() < 8
        {
            return Err(IdeviceError::UnexpectedResponse);
        }
        Ok(u64::from_le_bytes(
            res.header_payload[..8].try_into().unwrap(),
        ))
    }

    /// One Write packet carrying all of `data` (Swift's FFI
    /// `afc_file_write` is a single call; D-M5-15).
    pub async fn file_write(
        &mut self,
        handle: u64,
        data: &[u8],
    ) -> Result<(), IdeviceError> {
        self.exchange(
            AfcOpcode::Write,
            handle.to_le_bytes().to_vec(),
            data.to_vec(),
        )
        .await?;
        Ok(())
    }

    /// Reads a whole file: Read requests until a short Data
    /// answer ends the file. `AfcClient::read` decodes a
    /// non-success Status into `IdeviceError::Afc`, so an
    /// EndOfData status arrives as that error and is treated
    /// as EOF here.
    pub async fn file_read_entire(
        &mut self,
        handle: u64,
    ) -> Result<Vec<u8>, IdeviceError> {
        let mut out = Vec::new();
        loop {
            let mut header_payload = handle.to_le_bytes().to_vec();
            header_payload.extend_from_slice(&READ_CHUNK.to_le_bytes());
            let packet = self.packet(AfcOpcode::Read, header_payload, Vec::new());
            self.client.send(packet).await?;
            match self.client.read().await {
                Ok(res) => match res.header.operation {
                    AfcOpcode::Data => {
                        let short = (res.payload.len() as u64) < READ_CHUNK;
                        out.extend_from_slice(&res.payload);
                        if short {
                            return Ok(out);
                        }
                    }
                    // A success Status packet: no more data.
                    AfcOpcode::Status => return Ok(out),
                    _ => return Err(IdeviceError::UnexpectedResponse),
                },
                Err(IdeviceError::Afc(AfcError::EndOfData)) => return Ok(out),
                Err(e) => return Err(e),
            }
        }
    }

    pub async fn file_close(&mut self, handle: u64) -> Result<(), IdeviceError> {
        self.exchange(
            AfcOpcode::FileClose,
            handle.to_le_bytes().to_vec(),
            Vec::new(),
        )
        .await?;
        Ok(())
    }
}

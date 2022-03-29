use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::convert::TryInto;
use std::io::Cursor;
use std::mem::size_of;
use std::net::SocketAddr;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;

#[derive(Serialize, Deserialize, Debug)]
pub struct Msg {
    pub addr: AddrPair,
    pub typ: MsgType,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MsgType {
    MapData(Vec<u8>),
    MapConnecting,
    MapConnected,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AddrPair {
    pub local_addr: SocketAddr,
    pub lst_addr: SocketAddr,
    pub remap_addr: SocketAddr,
    pub dst_addr: SocketAddr,
}

// TODO: Should resize vec before read.
pub struct MsgCtx<RM, WM> {
    read_size: Option<usize>,
    read_buf: Vec<u8>,
    msgs_from_read: VecDeque<RM>,

    written_size: Option<usize>,
    write_buf: Vec<u8>,
    msgs_to_write: VecDeque<WM>,
}

impl<RM, WM> MsgCtx<RM, WM> {
    pub fn new() -> Self {
        MsgCtx {
            read_size: None,
            read_buf: Vec::with_capacity(2048),
            msgs_from_read: VecDeque::new(),

            written_size: None,
            write_buf: Vec::with_capacity(2048),
            msgs_to_write: VecDeque::new(),
        }
    }

    pub fn have_rx_msg(&self) -> bool {
        !self.msgs_from_read.is_empty()
    }

    pub fn pop_rx_msg(&mut self) -> Option<RM> {
        self.msgs_from_read.pop_front()
    }

    pub fn need_to_write(&self) -> bool {
        !self.msgs_to_write.is_empty() || self.written_size.is_some()
    }

    pub fn queue_tx_msg(&mut self, msg: WM) {
        self.msgs_to_write.push_back(msg);
    }
}

impl<RM, WM> MsgCtx<RM, WM>
where
    RM: DeserializeOwned,
{
    pub fn handle_read(&mut self, readhalf: &mut OwnedReadHalf) -> Result<(), std::io::Error> {
        if self.read_size.is_none() {
            let len = self.read_buf.len();
            readhalf.try_read(&mut self.read_buf[len..size_of::<u64>()])?;
            if self.read_buf.len() != size_of::<u64>() {
                // Need to read more.
                return Ok(());
            }
            self.read_size =
                Some(u64::from_be_bytes(self.read_buf[..].try_into().unwrap()) as usize);
            self.read_buf.clear();
        }
        let read_size = self.read_size.unwrap();
        if read_size > 1024 * 1024 * 100 {
            eprintln!("read_size is too large: {}", read_size);
            self.read_size = None;
            self.read_buf.clear();
            return Err(std::io::Error::from_raw_os_error(22));
        }

        // Reserve enough space for data.
        if self.read_buf.capacity() < read_size {
            self.read_buf.reserve(read_size - self.read_buf.len());
        }

        // Now try to read data.
        let len = self.read_buf.len();
        readhalf.try_read(&mut self.read_buf[len..read_size])?;
        if self.read_buf.len() != read_size {
            // Need more data.
            return Ok(());
        }

        // Deserialize.
        let res = match bincode::deserialize::<RM>(&self.read_buf[..]) {
            Ok(msg) => {
                self.msgs_from_read.push_back(msg);
                Ok(())
            }
            Err(e) => {
                eprintln!("Failed to deserialize: {}", e);
                Err(std::io::Error::from_raw_os_error(22))
            }
        };
        self.read_size = None;
        self.read_buf.clear();
        res
    }

    pub async fn read(&mut self, readhalf: &mut OwnedReadHalf) -> Result<RM, std::io::Error> {
        if let Some(rmsg) = self.pop_rx_msg() {
            return Ok(rmsg);
        }
        while !self.have_rx_msg() {
            let _ = readhalf.readable().await;
            self.handle_read(readhalf)?;
        }
        // There must be msg received here.
        Ok(self.pop_rx_msg().unwrap())
    }
}

impl<RM, WM> MsgCtx<RM, WM>
where
    WM: Serialize,
{
    pub fn handle_write(&mut self, writehalf: &mut OwnedWriteHalf) -> Result<(), std::io::Error> {
        if self.written_size.is_none() {
            // Serialize first.
            let msg = match self.msgs_to_write.pop_front() {
                Some(msg) => msg,
                None => return Ok(()),
            };
            self.write_buf.clear();
            let mut cursor = Cursor::new(&mut self.write_buf);
            // Reserve space for size of msg.
            cursor.set_position(size_of::<u64>() as u64);
            if let Err(e) = bincode::serialize_into(&mut cursor, &msg) {
                eprintln!("Failed to serialise: {}", e);
                drop(cursor);
                self.msgs_to_write.push_front(msg);
                return Err(std::io::Error::from_raw_os_error(22));
            }
            drop(cursor);
            // Setup size for msg.
            let size = (self.write_buf.len() - size_of::<u64>()) as u64;
            let mut size = size.to_be_bytes();
            (&mut self.write_buf[0..size_of::<u64>()]).copy_from_slice(&mut size[..]);
            self.written_size = Some(0);
        }
        // Now try to write to socket.
        let mut written_size = self.written_size.unwrap();
        written_size += match writehalf.try_write(&self.write_buf[written_size..]) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Failed to write: {}", e);
                self.written_size = None;
                self.write_buf.clear();
                return Err(e);
            }
        };
        if written_size == self.write_buf.len() {
            // All data is written.
            self.written_size = None;
            self.write_buf.clear();
        }
        Ok(())
    }

    pub async fn write(
        &mut self,
        writehalf: &mut OwnedWriteHalf,
        wmsg: WM,
    ) -> Result<(), std::io::Error> {
        self.queue_tx_msg(wmsg);
        while self.need_to_write() {
            let _ = writehalf.writable().await;
            self.handle_write(writehalf)?;
        }
        Ok(())
    }
}

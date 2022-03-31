use log::{error, trace};
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
    pub dir: MsgDirection,
    pub typ: MsgType,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum MsgDirection {
    L2D,
    D2L,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MsgType {
    MapData(Vec<u8>),
    MapConnecting,
    MapConnected,
    MapDisconnect,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AddrPair {
    pub local_addr: SocketAddr,
    pub lst_addr: SocketAddr,
    pub remap_addr: SocketAddr,
    pub dst_addr: SocketAddr,
}

pub struct MsgCtx<RM, WM> {
    frag_size: Option<usize>,
    read_size: usize,
    read_buf: Vec<u8>,
    msgs_from_read: VecDeque<RM>,

    written_size: Option<usize>,
    write_buf: Vec<u8>,
    msgs_to_write: VecDeque<WM>,
}

impl<RM, WM> MsgCtx<RM, WM> {
    pub fn new() -> Self {
        MsgCtx {
            frag_size: None,
            read_size: 0,
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

    pub fn reset_read(&mut self) {
        self.frag_size = None;
        self.read_size = 0;
        self.read_buf.clear();
    }

    pub fn reset_write(&mut self) {
        self.written_size = None;
        self.write_buf.clear();
    }
}

impl<RM, WM> MsgCtx<RM, WM>
where
    RM: DeserializeOwned,
{
    pub fn handle_read(&mut self, readhalf: &mut OwnedReadHalf) -> Result<(), std::io::Error> {
        if self.frag_size.is_none() {
            // Read size of fragment.
            if self.read_buf.len() < size_of::<u64>() {
                self.read_buf.resize(size_of::<u64>(), 0);
            }
            self.read_size +=
                match readhalf.try_read(&mut self.read_buf[self.read_size..size_of::<u64>()]) {
                    Ok(s) => {
                        if s == 0 {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::NotConnected,
                                "Read zero byte",
                            ));
                        } else {
                            s
                        }
                    }
                    Err(e) => {
                        trace!("{}:{} Failed to read: {}", file!(), line!(), e);
                        return Err(e);
                    }
                };
            assert!(self.read_size <= size_of::<u64>());
            if self.read_size < size_of::<u64>() {
                // Need to read more.
                return Ok(());
            }
            self.frag_size =
                Some(u64::from_be_bytes(self.read_buf[..].try_into().unwrap()) as usize);
            self.read_size = 0;
            self.read_buf.clear();
            trace!(
                "{}:{} frag_size:{}",
                file!(),
                line!(),
                self.frag_size.unwrap()
            );

            if self.frag_size.unwrap() > 1024 * 1024 * 100 {
                error!(
                    "{}:{} read_size is too large: {}",
                    file!(),
                    line!(),
                    self.frag_size.unwrap()
                );
                return Err(std::io::Error::from_raw_os_error(22));
            }
            self.read_buf.resize(self.frag_size.unwrap(), 0);
        }

        let frag_size = self.frag_size.unwrap();
        self.read_size += match readhalf.try_read(&mut self.read_buf[self.read_size..frag_size]) {
            Ok(s) => {
                if s == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotConnected,
                        "Read zero byte",
                    ));
                } else {
                    s
                }
            }
            Err(e) => {
                trace!("{}:{} Failed to read: {}", file!(), line!(), e);
                return Err(e);
            }
        };
        trace!(
            "{}:{} read {}/{}",
            file!(),
            line!(),
            self.read_size,
            frag_size
        );
        assert!(self.read_size <= frag_size);
        if self.read_size < frag_size {
            // Need more data.
            return Ok(());
        }

        // Deserialize.
        let res = match bincode::deserialize::<RM>(&self.read_buf[..]) {
            Ok(msg) => {
                trace!("{}:{} deserialize msg", file!(), line!());
                self.msgs_from_read.push_back(msg);
                Ok(())
            }
            Err(e) => {
                error!("{}:{} Failed to deserialize: {}", file!(), line!(), e);
                Err(std::io::Error::from_raw_os_error(22))
            }
        };
        self.frag_size = None;
        self.read_size = 0;
        self.read_buf.clear();
        res
    }

    pub async fn read(&mut self, readhalf: &mut OwnedReadHalf) -> Result<RM, std::io::Error> {
        if let Some(rmsg) = self.pop_rx_msg() {
            trace!("{}:{} return exist msg", file!(), line!());
            return Ok(rmsg);
        }
        while !self.have_rx_msg() {
            let _ = readhalf.readable().await;
            match self.handle_read(readhalf) {
                Ok(_) => {}
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::WouldBlock {
                        self.reset_read();
                        return Err(e);
                    }
                }
            }
        }
        trace!("{}:{} Read msg successfully", file!(), line!());
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
                error!("{}:{} Failed to serialise: {}", file!(), line!(), e);
                drop(cursor);
                self.msgs_to_write.push_front(msg);
                return Err(std::io::Error::from_raw_os_error(22));
            }
            drop(cursor);
            // Setup size for msg.
            let size = (self.write_buf.len() - size_of::<u64>()) as u64;
            trace!("{}:{} Write msg size {}", file!(), line!(), size);
            let mut size = size.to_be_bytes();
            (&mut self.write_buf[0..size_of::<u64>()]).copy_from_slice(&mut size[..]);
            self.written_size = Some(0);
        }
        // Now try to write to socket.
        let mut written_size = self.written_size.unwrap();
        trace!(
            "{}:{} Write before {}/{}",
            file!(),
            line!(),
            written_size,
            self.write_buf.len()
        );
        written_size += match writehalf.try_write(&self.write_buf[written_size..]) {
            Ok(r) => r,
            Err(e) => {
                error!("{}:{} Failed to write: {}", file!(), line!(), e);
                return Err(e);
            }
        };
        trace!(
            "{}:{} Write after {}/{}",
            file!(),
            line!(),
            written_size,
            self.write_buf.len()
        );
        if written_size == self.write_buf.len() {
            // All data is written.
            self.written_size = None;
            self.write_buf.clear();
        } else {
            self.written_size = Some(written_size);
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
            match self.handle_write(writehalf) {
                Ok(_) => {}
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::WouldBlock {
                        self.reset_write();
                        return Err(e);
                    }
                }
            };
        }
        trace!("{}:{} Written msg", file!(), line!());
        Ok(())
    }
}

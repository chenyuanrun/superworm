use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::convert::TryInto;
use std::io::Cursor;
use std::mem::size_of;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;

#[derive(Serialize, Deserialize, Debug)]
pub enum Msg {}

pub struct ReadCtx {
    pub read_size: Option<usize>,
    pub read_buf: Vec<u8>,
    pub msgs: VecDeque<Msg>,
}

impl ReadCtx {
    pub fn new() -> Self {
        ReadCtx {
            read_size: None,
            read_buf: Vec::with_capacity(2048),
            msgs: VecDeque::new(),
        }
    }

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
            println!("read_size is too large: {}", read_size);
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
        let res = match bincode::deserialize::<Msg>(&self.read_buf[..]) {
            Ok(msg) => {
                self.msgs.push_back(msg);
                Ok(())
            }
            Err(e) => {
                println!("Failed to deserialize: {}", e);
                Err(std::io::Error::from_raw_os_error(22))
            }
        };
        self.read_size = None;
        self.read_buf.clear();
        res
    }
}

pub struct WriteCtx {
    pub written_size: Option<usize>,
    pub write_buf: Vec<u8>,
    pub msgs: VecDeque<Msg>,
}

impl WriteCtx {
    pub fn new() -> Self {
        WriteCtx {
            written_size: None,
            write_buf: Vec::with_capacity(2048),
            msgs: VecDeque::new(),
        }
    }

    pub fn handle_write(&mut self, writehalf: &mut OwnedWriteHalf) -> Result<(), std::io::Error> {
        if self.written_size.is_none() {
            // Serialize first.
            let msg = match self.msgs.pop_front() {
                Some(msg) => msg,
                None => return Ok(()),
            };
            self.write_buf.clear();
            let mut cursor = Cursor::new(&mut self.write_buf);
            // Reserve space for size of msg.
            cursor.set_position(size_of::<u64>() as u64);
            if let Err(e) = bincode::serialize_into(&mut cursor, &msg) {
                println!("Failed to serialise: {}", e);
                drop(cursor);
                self.msgs.push_front(msg);
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
                println!("Failed to write: {}", e);
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
}

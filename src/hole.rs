use crate::msg::Msg;
use std::collections::VecDeque;
use std::convert::TryInto;
use std::io::Cursor;
use std::mem::size_of;
use std::net::SocketAddr;
use std::panic;
use tokio::io::Interest;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, Permit};
use tokio::time::{sleep, Duration};

pub async fn hole(eps: Vec<SocketAddr>) {
    if eps.len() != 2 {
        println!("Expect 2 ep, found {}", eps.len());
        return;
    }
    let (tx1, rx1) = mpsc::channel(1024);
    let (tx2, rx2) = mpsc::channel(1024);

    tokio::spawn(route(eps[0].clone(), tx1, rx2));
    tokio::spawn(route(eps[1].clone(), tx2, rx1));

    loop {
        sleep(Duration::from_secs(1)).await;
        println!("tick");
        // TODO: exit gracefully.
    }
}

struct Hole {
    read_size: Option<usize>,
    read_buf: Vec<u8>,
    msgs_from_ep: VecDeque<Msg>,

    written_size: Option<usize>,
    write_buf: Vec<u8>,
    msgs_from_chan: VecDeque<Msg>,
}

impl Hole {
    fn new() -> Self {
        Hole {
            read_size: None,
            read_buf: Vec::with_capacity(2048),
            msgs_from_ep: VecDeque::with_capacity(64),

            written_size: None,
            write_buf: Vec::new(),
            msgs_from_chan: VecDeque::with_capacity(64),
        }
    }

    fn handle_read(&mut self, readhalf: &mut OwnedReadHalf) -> Result<(), std::io::Error> {
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
                self.msgs_from_ep.push_back(msg);
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

    fn handle_write(&mut self, writehalf: &mut OwnedWriteHalf) -> Result<(), std::io::Error> {
        if self.written_size.is_none() {
            // Serialize first.
            let msg = match self.msgs_from_chan.pop_front() {
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
                self.msgs_from_chan.push_front(msg);
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

    fn handle_msg_recv(&mut self, msg: Msg) {
        self.msgs_from_chan.push_back(msg);
    }

    fn handle_msg_send(&mut self, permit: Permit<'_, Msg>) {
        if let Some(msg) = self.msgs_from_ep.pop_front() {
            permit.send(msg);
        }
    }

    fn should_write(&self) -> bool {
        self.written_size.is_some() || self.msgs_from_chan.len() != 0
    }

    fn should_send(&self) -> bool {
        self.msgs_from_ep.len() != 0
    }
}

async fn route(ep: SocketAddr, tx: mpsc::Sender<Msg>, mut rx: mpsc::Receiver<Msg>) {
    let mut hole = Hole::new();
    // Conntect to endpoint.
    let (mut readhalf, mut writehalf) = connect(ep).await.into_split();

    loop {
        tokio::select! {
            // Read from socket.
            r = readhalf.readable() => {
                if let Err(e) = hole.handle_read(&mut readhalf) {
                    println!{"Failed to handle read from {}: {}", ep, e};
                    let (rh, wh) = connect(ep).await.into_split();
                    readhalf = rh;
                    writehalf = wh;
                }
            }
            // Write to socket.
            r = writehalf.writable(), if hole.should_write() => {
                if let Err(e) = hole.handle_write(&mut writehalf) {
                    println!{"Failed to handle write from {}: {}", ep, e};
                    let (rh, wh) = connect(ep).await.into_split();
                    readhalf = rh;
                    writehalf = wh;
                }
            }
            // Read from channel.
            r = rx.recv() => {
                match r {
                    Some(msg) => {
                        hole.handle_msg_recv(msg);
                    },
                    None => {
                        panic!{"Receiver closed for {}", ep};
                    },
                }
            }
            // Write to channel.
            r = tx.reserve(), if hole.should_send() => {
                match r {
                    Ok(permit) => {
                        hole.handle_msg_send(permit);
                    },
                    Err(e) => {
                        panic!{"Sender closed for {}: {}", ep, e};
                    }
                }
            }
        }
    }
}

async fn connect(addr: SocketAddr) -> TcpStream {
    loop {
        match TcpStream::connect(addr).await {
            Ok(conn) => {
                return conn;
            }
            Err(e) => {
                println!("Failed to connect to {}: {}", addr, e);
                // Try again in a second.
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

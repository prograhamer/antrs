use super::Message;
use crate::node;
use core::time::Duration;
use std::sync::Mutex;
use std::thread;

pub struct Publisher<'reader> {
    reader: Mutex<&'reader (dyn node::Reader + Sync)>,
    buffer: Vec<u8>,
    read_index: usize,
    write_index: usize,
    sender: crossbeam_channel::Sender<Message>,
}

impl Publisher<'_> {
    pub fn new(
        reader: &(dyn node::Reader + Sync),
        sender: crossbeam_channel::Sender<Message>,
        buffer_size: usize,
    ) -> Publisher {
        Publisher {
            reader: reader.into(),
            sender,
            buffer: vec![0u8; buffer_size],
            read_index: 0,
            write_index: 0,
        }
    }

    pub fn run(&mut self) {
        loop {
            let read_size;

            {
                read_size = match self.reader.lock().unwrap().read(
                    &mut self.buffer[self.write_index..],
                    Duration::new(0, 100_000_000),
                ) {
                    Ok(size) => size,
                    Err(e) => {
                        if e != crate::node::Error::Timeout {
                            panic!("read from endpoint: {}", e)
                        } else {
                            0
                        }
                    }
                };
                self.write_index += read_size;
            }

            if read_size > 0 {
                let mut discard_count = 0usize;
                while self.buffer[self.read_index] != super::SYNC
                    && self.read_index < self.write_index
                {
                    self.read_index += 1;
                    discard_count += 1;
                }

                if discard_count > 0 {
                    println!("discarded {} bytes!", discard_count);
                }

                while self.read_index < self.write_index - 5 {
                    let msg = match Message::decode(&self.buffer[self.read_index..]) {
                        Ok(msg) => msg,
                        Err(e) => panic!("decoding message: {}", e),
                    };

                    self.read_index += msg.encoded_len();

                    self.sender.send(msg).expect("send should work");
                }

                if self.read_index == self.write_index {
                    self.read_index = 0;
                    self.write_index = 0;
                } else if self.read_index > 0 {
                    let offset = self.write_index - self.read_index;

                    for i in 0..offset {
                        self.buffer[i] = self.buffer[self.read_index + i];
                    }

                    self.read_index = 0;
                    self.write_index = offset;
                }
            }

            thread::sleep(Duration::new(0, 10_000_000));
        }
    }
}

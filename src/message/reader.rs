use crate::node;
use core::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Debug)]
pub enum Error {
    ReadError(crate::node::Error),
    DecodeError(super::Error),
}

impl From<crate::node::Error> for Error {
    fn from(value: crate::node::Error) -> Self {
        Self::ReadError(value)
    }
}

impl From<super::Error> for Error {
    fn from(value: super::Error) -> Self {
        Self::DecodeError(value)
    }
}

struct Buffer {
    data: Vec<u8>,
    read_index: usize,
    write_index: usize,
}

pub struct Publisher<'reader> {
    reader: Mutex<&'reader (dyn node::Reader + Sync)>,
    buffer: Mutex<Buffer>,
    sender: crossbeam_channel::Sender<super::Message>,
    request_stop: AtomicBool,
}

impl Publisher<'_> {
    pub fn new(
        reader: &(dyn node::Reader + Sync),
        sender: crossbeam_channel::Sender<super::Message>,
        buffer_size: usize,
    ) -> Publisher {
        Publisher {
            reader: reader.into(),
            sender,
            buffer: Mutex::new(Buffer {
                data: vec![0u8; buffer_size],
                read_index: 0,
                write_index: 0,
            }),
            request_stop: AtomicBool::new(false),
        }
    }

    pub fn stop(&self) {
        self.request_stop.store(true, Ordering::SeqCst);
    }

    pub fn run(&self) -> Result<(), Error> {
        let mut buffer = self.buffer.lock().unwrap();

        loop {
            if self.request_stop.load(Ordering::SeqCst) {
                return Ok(());
            }

            let read_size;

            {
                let write_index = buffer.write_index;
                read_size = match self.reader.lock().unwrap().read(
                    &mut buffer.data[write_index..],
                    Duration::new(0, 100_000_000),
                ) {
                    Ok(size) => size,
                    Err(crate::node::Error::Timeout) => 0,
                    Err(e) => return Err(e.into()),
                };
                buffer.write_index += read_size;
            }

            if read_size > 0 {
                let mut discard_count = 0usize;
                while buffer.data[buffer.read_index] != super::SYNC
                    && buffer.read_index < buffer.write_index
                {
                    buffer.read_index += 1;
                    discard_count += 1;
                }

                if discard_count > 0 {
                    println!("discarded {} bytes!", discard_count);
                }

                while buffer.read_index + 5 < buffer.write_index {
                    let msg = match super::Message::decode(
                        &buffer.data[buffer.read_index..buffer.write_index],
                    ) {
                        Ok(msg) => msg,
                        Err(super::Error::InsufficientData) => {
                            break;
                        }
                        Err(e) => return Err(e.into()),
                    };

                    buffer.read_index += msg.encoded_len();

                    self.sender.send(msg).expect("send should work");
                }

                if buffer.read_index == buffer.write_index {
                    buffer.read_index = 0;
                    buffer.write_index = 0;
                } else if buffer.read_index > 0 {
                    let offset = buffer.write_index - buffer.read_index;

                    for i in 0..offset {
                        buffer.data[i] = buffer.data[buffer.read_index + i];
                    }

                    buffer.read_index = 0;
                    buffer.write_index = offset;
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use crossbeam_channel::RecvTimeoutError;

    use crate::message::{Message, MessageCode, MessageID, SYNC};
    use crate::node;

    struct MockReader {
        buffers: Vec<Vec<u8>>,
        byte_index: Mutex<usize>,
        buffer_index: Mutex<usize>,
    }

    impl MockReader {
        fn new(buffers: Vec<Vec<u8>>) -> MockReader {
            MockReader {
                buffers,
                byte_index: Mutex::new(0),
                buffer_index: Mutex::new(0),
            }
        }

        fn complete(&self) -> bool {
            let buffer_index = self.buffer_index.lock().unwrap();
            *buffer_index == self.buffers.len()
        }
    }

    impl node::Reader for MockReader {
        fn read(
            &self,
            buf: &mut [u8],
            _timeout: std::time::Duration,
        ) -> Result<usize, crate::node::Error> {
            let mut buffer_index = self.buffer_index.lock().unwrap();
            let mut size = 0;

            if let Some(buffer) = self.buffers.get(*buffer_index) {
                let mut byte_index = self.byte_index.lock().unwrap();
                for (i, e) in buf.iter_mut().enumerate() {
                    if *byte_index + i >= buffer.len() {
                        break;
                    }

                    *e = buffer[*byte_index + i];
                    size += 1;
                }

                *byte_index += size;

                if *byte_index == buffer.len() {
                    *buffer_index += 1;
                    *byte_index = 0;
                }
            } else {
                return Err(node::Error::Timeout);
            }

            Ok(size)
        }
    }

    fn run_test(buffers: Vec<Vec<u8>>) -> Result<Vec<Message>, String> {
        let messages = Arc::new(Mutex::new(vec![]));
        let (sender, receiver) = crossbeam_channel::unbounded();
        let stop = Arc::new(AtomicBool::new(false));

        let reader = MockReader::new(buffers);

        thread::scope(|s| {
            let receiver_handle;
            {
                let stop = Arc::clone(&stop);
                let messages = Arc::clone(&messages);
                receiver_handle = s.spawn(move || loop {
                    if receiver.len() == 0 && stop.load(Ordering::SeqCst) {
                        break;
                    }

                    match receiver.recv_timeout(Duration::from_millis(10)) {
                        Ok(message) => {
                            let mut messages = messages.lock().unwrap();
                            messages.push(message);
                        }
                        Err(RecvTimeoutError::Disconnected) => panic!("receiver disconnected"),
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                });
            }

            let publisher = Arc::new(super::Publisher::new(&reader, sender, 128));

            let publisher_handle;
            {
                let publisher = Arc::clone(&publisher);
                publisher_handle = s.spawn(move || publisher.run());
            }

            // Wait until all buffers are read from the mock reader
            while !reader.complete() {
                thread::sleep(Duration::from_millis(1));
            }

            publisher.stop();

            // Join publisher thread - when this completes, we know all messages have been read
            // and published
            if let Err(e) = publisher_handle
                .join()
                .expect("publisher thread shouldn't panic")
            {
                panic!("publisher run returned error: {:?}", e);
            }

            stop.store(true, Ordering::SeqCst);

            if receiver_handle.join().is_err() {
                return Err("receiver thread shouldn't panic");
            }

            Ok(())
        })?;

        let messages = messages.lock().unwrap().to_owned();
        Ok(messages)
    }

    #[test]
    fn it_parses_single_message() {
        let buffer = vec![
            SYNC,
            0x03,
            MessageID::ChannelResponseEvent.into(),
            0x00,
            MessageID::SetNetworkKey.into(),
            MessageCode::ResponseNoError.into(),
            0xa1,
        ];

        match run_test(vec![buffer]) {
            Ok(messages) => {
                assert_eq!(messages.len(), 1);
                assert_eq!(
                    messages.get(0),
                    Some(&Message::ChannelResponseEvent(
                        crate::message::ChannelResponseEventData {
                            channel: 0,
                            message_id: MessageID::SetNetworkKey,
                            message_code: MessageCode::ResponseNoError
                        }
                    ))
                )
            }
            Err(e) => panic!("test run raised an error: {}", e),
        }
    }

    #[test]
    fn it_parses_two_messages() {
        let buffer = vec![
            SYNC,
            9,
            MessageID::SetNetworkKey.into(),
            0,
            9,
            8,
            7,
            6,
            5,
            4,
            3,
            2,
            235,
            SYNC,
            0x03,
            MessageID::ChannelResponseEvent.into(),
            0x00,
            MessageID::SetNetworkKey.into(),
            MessageCode::ResponseNoError.into(),
            0xa1,
        ];

        match run_test(vec![buffer]) {
            Ok(messages) => {
                assert_eq!(messages.len(), 2);
                assert_eq!(
                    messages.get(0),
                    Some(&Message::SetNetworkKey(crate::message::SetNetworkKeyData {
                        network: 0,
                        key: [9, 8, 7, 6, 5, 4, 3, 2]
                    }))
                );
                assert_eq!(
                    messages.get(1),
                    Some(&Message::ChannelResponseEvent(
                        crate::message::ChannelResponseEventData {
                            channel: 0,
                            message_id: MessageID::SetNetworkKey,
                            message_code: MessageCode::ResponseNoError
                        }
                    ))
                );
            }
            Err(e) => {
                panic!("error returned by test run: {}", e);
            }
        }
    }

    #[test]
    fn it_parses_partial_message() {
        let buffer = vec![SYNC, 9, MessageID::SetNetworkKey.into(), 0, 9, 8, 7, 6];

        match run_test(vec![buffer]) {
            Ok(messages) => {
                assert_eq!(messages.len(), 0);
            }
            Err(e) => panic!("error returned by test run: {}", e),
        }
    }

    #[test]
    fn it_parses_complete_message_followed_by_partial() {
        let buffer = vec![
            SYNC,
            9,
            MessageID::SetNetworkKey.into(),
            0,
            9,
            8,
            7,
            6,
            5,
            4,
            3,
            2,
            235,
            SYNC,
            0x03,
            MessageID::ChannelResponseEvent.into(),
            0x00,
        ];
        match run_test(vec![buffer]) {
            Ok(messages) => {
                assert_eq!(messages.len(), 1);
                assert_eq!(
                    messages.get(0),
                    Some(&Message::SetNetworkKey(crate::message::SetNetworkKeyData {
                        network: 0,
                        key: [9, 8, 7, 6, 5, 4, 3, 2]
                    }))
                );
            }
            Err(e) => panic!("error returned by test run: {}", e),
        }
    }

    #[test]
    fn it_parses_single_message_split_over_multiple_reads() {
        let buffers = vec![
            vec![SYNC, 9],
            vec![MessageID::SetNetworkKey.into(), 0, 9, 8, 7],
            vec![6, 5, 4, 3, 2, 235],
        ];

        match run_test(buffers) {
            Ok(messages) => {
                assert_eq!(messages.len(), 1);
                assert_eq!(
                    messages.get(0),
                    Some(&Message::SetNetworkKey(crate::message::SetNetworkKeyData {
                        network: 0,
                        key: [9, 8, 7, 6, 5, 4, 3, 2]
                    }))
                );
            }
            Err(e) => panic!("error returned by test run: {}", e),
        }
    }
}

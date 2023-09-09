use std::collections::LinkedList;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use antrs::message::{
    AssignChannelData, ChannelExtendedAssignment, ChannelType, Message, MessageCode, MessageID,
    OpenChannelData, SetChannelIDData, SetChannelRFFrequencyData, SetNetworkKeyData,
};
use antrs::node;

fn main() -> Result<(), node::Error> {
    let key: [u8; 8] = match std::env::var("ANT_NETWORK_KEY") {
        Ok(key) => match hex::decode(key) {
            Ok(key) => match key.try_into() {
                Ok(key) => key,
                Err(v) => panic!("invalid value for ANT_NETWORK_KEY: {:?}", v),
            },
            Err(e) => panic!("invalid value for ANT_NETWORK_KEY: {}", e),
        },
        Err(_) => panic!("no value for ANT_NETWORK_KEY in environment"),
    };

    let mut node = node::NodeBuilder::new().build();

    node.open()?;

    let receiver = Receiver::new(node.receive_messages());
    let join_handle = receiver.start();

    let reset = Message::ResetSystem;
    node.write_message(reset, Duration::from_secs(1))?;
    thread::sleep(Duration::from_secs(1));

    let set_network_key = Message::SetNetworkKey(SetNetworkKeyData { network: 0, key });
    node.write_message(set_network_key, Duration::from_secs(1))?;
    wait_for_channel_response_event(&receiver, 0, MessageID::SetNetworkKey);

    let assign_channel = Message::AssignChannel(AssignChannelData {
        channel: 0,
        channel_type: ChannelType::Receive,
        network: 0,
        extended_assignment: ChannelExtendedAssignment::BACKGROUND_SCANNING,
    });
    node.write_message(assign_channel, Duration::from_secs(1))?;
    wait_for_channel_response_event(&receiver, 0, MessageID::AssignChannel);

    let set_channel_id = Message::SetChannelID(SetChannelIDData {
        channel: 0,
        device: 0,
        pairing: false,
        device_type: 0,
        transmission_type: 0,
    });
    node.write_message(set_channel_id, Duration::from_secs(1))?;
    wait_for_channel_response_event(&receiver, 0, MessageID::SetChannelID);

    let set_channel_rf_freq = Message::SetChannelRFFrequency(SetChannelRFFrequencyData {
        channel: 0,
        frequency: 57,
    });
    node.write_message(set_channel_rf_freq, Duration::from_secs(1))?;
    wait_for_channel_response_event(&receiver, 0, MessageID::SetChannelRFFrequency);

    let open_channel = Message::OpenChannel(OpenChannelData { channel: 0 });
    node.write_message(open_channel, Duration::from_secs(1))?;

    thread::sleep(Duration::new(30, 0));

    receiver.stop();
    join_handle.join().unwrap();

    Ok(())
}

fn wait_for_channel_response_event(receiver: &Receiver, channel: u8, message_id: MessageID) {
    match receiver.wait_for_channel_response_event(
        channel,
        message_id,
        MessageCode::ResponseNoError,
        Duration::from_millis(100),
    ) {
        Ok(_) => println!("wait for channel response: {}: received", message_id),
        Err(e) => panic!("wait of channel response: {}: {}", message_id, e),
    }
}

struct Receiver {
    messages: Arc<RwLock<LinkedList<Message>>>,
    rx: Arc<Mutex<mpsc::Receiver<Message>>>,
    request_stop: Arc<AtomicBool>,
}

impl Receiver {
    fn new(rx: mpsc::Receiver<Message>) -> Receiver {
        Receiver {
            messages: Arc::new(RwLock::new(LinkedList::new())),
            rx: Arc::new(Mutex::new(rx)),
            request_stop: Arc::new(AtomicBool::new(false)),
        }
    }

    fn start(&self) -> thread::JoinHandle<()> {
        let rx_mutex = Arc::clone(&self.rx);
        let messages_mutex = Arc::clone(&self.messages);
        let request_stop = Arc::clone(&self.request_stop);

        thread::Builder::new()
            .name(String::from("receiver"))
            .spawn(move || loop {
                let rx = rx_mutex.lock().unwrap();

                match rx.try_recv() {
                    Ok(msg) => {
                        println!("received: {}", msg);
                        let mut messages = messages_mutex.write().unwrap();
                        messages.push_back(msg);
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => {
                        println!("receiver disconnected, exiting");
                        break;
                    }
                }

                if request_stop.load(Ordering::SeqCst) {
                    println!("stop requesting, exiting");
                    break;
                }

                thread::sleep(Duration::from_millis(10));
            })
            .unwrap()
    }

    fn stop(&self) {
        self.request_stop.store(true, Ordering::SeqCst);
    }

    fn wait_for_channel_response_event(
        &self,
        channel: u8,
        message_id: MessageID,
        message_code: MessageCode,
        timeout: Duration,
    ) -> Result<(), String> {
        let start = std::time::Instant::now();
        loop {
            if std::time::Instant::now().duration_since(start) > timeout {
                return Err(String::from("timeout"));
            }

            {
                let messages = self.messages.write().unwrap();

                for message in messages.iter() {
                    if let Message::ChannelResponseEvent(data) = message {
                        if data.channel == channel && data.message_id == message_id {
                            if data.message_code == message_code {
                                return Ok(());
                            } else {
                                return Err(format!(
                                    "unexpected message code: {}",
                                    data.message_code
                                ));
                            }
                        }
                    }
                }
            };

            thread::sleep(Duration::from_millis(10));
        }
    }
}

mod capabilities;

use core::time::Duration;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Instant;

use crate::device::Device;
use crate::message::{self, reader, Message, MessageCode, MessageID, RequestMessageData};

#[derive(Debug, PartialEq)]
pub enum Error {
    DeviceNotFound,
    DeviceNotInitialized,
    EndpointNotInitialized,
    HandleNotInitialized,
    EndpointNotFound,
    Timeout,
    USBError(rusb::Error),
    ChannelResponseError,
    NoAvailableChannel,
    CapabilitiesNotInitialized,
}

impl From<rusb::Error> for Error {
    fn from(value: rusb::Error) -> Self {
        match value {
            rusb::Error::Timeout => Error::Timeout,
            _ => Error::USBError(value),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

const DYNASTREAM_INNOVATIONS_VID: u16 = 0xfcf;
const DI_ANT_M_STICK: u16 = 0x1009;

#[derive(Clone, Copy)]
struct Endpoint {
    interface: u8,
    address: u8,
}

pub struct Node {
    capabilities: Option<capabilities::Capabilities>,
    network_key: [u8; 8],
    vendor_id: u16,
    product_id: u16,
    device: Option<rusb::Device<rusb::GlobalContext>>,
    handle: Arc<RwLock<Option<rusb::DeviceHandle<rusb::GlobalContext>>>>,
    in_ep: Option<Endpoint>,
    out_ep: Option<Endpoint>,
    inbound_messages: Arc<RwLock<Vec<Message>>>,
    assigned: Arc<Mutex<HashMap<u8, Box<dyn Device + Send>>>>,
}

impl Node {
    pub fn open(&mut self) -> Result<(), Error> {
        self.device = Some(self.find_device()?);

        let (in_ep, out_ep) = self.find_endpoints()?;
        self.in_ep = Some(in_ep);
        self.out_ep = Some(out_ep);

        let mut handle = self
            .device
            .clone()
            .ok_or(Error::DeviceNotInitialized)?
            .open()?;

        handle.set_auto_detach_kernel_driver(true)?;
        handle.set_active_configuration(0)?;
        handle.claim_interface(in_ep.interface)?;
        if in_ep.interface != out_ep.interface {
            handle.claim_interface(out_ep.interface)?;
        }

        {
            let mut h = self.handle.write().unwrap();
            *h = Some(handle);
        }

        self.receive_messages()?;

        self.write_message(Message::ResetSystem, Duration::from_millis(100))?;
        thread::sleep(Duration::from_millis(2000));

        let set_network_key = Message::SetNetworkKey(message::SetNetworkKeyData {
            network: 0,
            key: self.network_key,
        });
        self.write_message(set_network_key, Duration::from_millis(100))?;
        self.expect_channel_response_no_error(
            0,
            MessageID::SetNetworkKey,
            Duration::from_millis(1000),
        )?;

        let request_capabilities = Message::RequestMessage(RequestMessageData {
            channel: 0,
            message_id: MessageID::Capabilities,
        });
        self.write_message(request_capabilities, Duration::from_millis(100))?;
        let matcher = |message: &Message| {
            if let Message::Capabilities(_) = message {
                true
            } else {
                false
            }
        };
        let capabilities = self.wait_for_message(matcher, Duration::from_millis(1000))?;
        if let Message::Capabilities(data) = capabilities {
            self.capabilities = Some(data.into())
        }

        Ok(())
    }

    pub fn close(&mut self) -> Result<(), Error> {
        let mut assigned = self.assigned.lock().unwrap();

        for channel in assigned.keys() {
            self.write_message(
                Message::CloseChannel(message::CloseChannelData { channel: *channel }),
                Duration::from_millis(100),
            )?;
            self.expect_channel_response_no_error(
                *channel,
                MessageID::CloseChannel,
                Duration::from_millis(1000),
            )?;
            let matcher = |message: &Message| {
                if let Message::ChannelResponseEvent(data) = message {
                    data.channel == *channel
                        && data.message_id == MessageID::ChannelEvent
                        && data.message_code == MessageCode::EventChannelClosed
                } else {
                    false
                }
            };
            self.wait_for_message(matcher, Duration::from_millis(1000))?;
        }
        assigned.clear();

        Ok(())
    }

    pub fn assign_channel(&mut self, device: Box<dyn Device + Send>) -> Result<(), Error> {
        let mut channel = None;
        let max_channels;

        if let Some(capabilities) = &self.capabilities {
            max_channels = capabilities.max_channels;
        } else {
            return Err(Error::CapabilitiesNotInitialized);
        }

        // TODO: make this locking prevent concurrent calls to assign_channel clobbering the same
        // channel
        {
            let assigned = self.assigned.lock().unwrap();

            for i in 0..max_channels {
                if !assigned.contains_key(&i) {
                    channel = Some(i);
                    break;
                }
            }
        }

        let channel = if let Some(channel) = channel {
            channel
        } else {
            return Err(Error::NoAvailableChannel);
        };

        let assign_channel = Message::AssignChannel(message::AssignChannelData {
            channel,
            channel_type: device.channel_type(),
            network: 0,
            extended_assignment: message::ChannelExtendedAssignment::empty(),
        });
        self.write_message(assign_channel, Duration::from_millis(100))?;
        self.expect_channel_response_no_error(
            channel,
            MessageID::AssignChannel,
            Duration::from_millis(100),
        )?;

        let pairing = device.pairing();
        let set_channel_id = Message::SetChannelID(message::SetChannelIDData {
            channel,
            device: pairing.device_id,
            pairing: false,
            device_type: device.device_type(),
            transmission_type: pairing.transmission_type,
        });
        self.write_message(set_channel_id, Duration::from_secs(100))?;
        self.expect_channel_response_no_error(
            channel,
            MessageID::SetChannelID,
            Duration::from_millis(100),
        )?;

        let set_channel_period = Message::SetChannelPeriod(message::SetChannelPeriodData {
            channel,
            period: device.channel_period(),
        });
        self.write_message(set_channel_period, Duration::from_millis(100))?;
        self.expect_channel_response_no_error(
            channel,
            MessageID::SetChannelPeriod,
            Duration::from_millis(100),
        )?;

        let set_channel_rf_freq =
            Message::SetChannelRFFrequency(message::SetChannelRFFrequencyData {
                channel,
                frequency: device.rf_frequency(),
            });
        self.write_message(set_channel_rf_freq, Duration::from_millis(100))?;
        self.expect_channel_response_no_error(
            channel,
            MessageID::SetChannelRFFrequency,
            Duration::from_millis(100),
        )?;

        let open_channel = Message::OpenChannel(message::OpenChannelData { channel });
        self.write_message(open_channel, Duration::from_millis(100))?;
        self.expect_channel_response_no_error(
            channel,
            MessageID::OpenChannel,
            Duration::from_millis(100),
        )?;

        {
            let mut assigned = self.assigned.lock().unwrap();
            assigned.insert(channel, device);
        }

        Ok(())
    }

    fn expect_channel_response_no_error(
        &self,
        channel: u8,
        message_id: MessageID,
        timeout: Duration,
    ) -> Result<(), Error> {
        let matcher = |message: &Message| {
            if let Message::ChannelResponseEvent(data) = message {
                data.channel == channel && data.message_id == message_id
            } else {
                false
            }
        };

        let message = self.wait_for_message(matcher, timeout)?;
        if let Message::ChannelResponseEvent(data) = message {
            if data.message_code == MessageCode::ResponseNoError {
                Ok(())
            } else {
                Err(Error::ChannelResponseError)
            }
        } else {
            unreachable!()
        }
    }

    fn wait_for_message<F>(&self, matcher: F, timeout: Duration) -> Result<Message, Error>
    where
        F: Fn(&Message) -> bool,
    {
        let start = Instant::now();

        // TODO: implement this with futures?
        loop {
            if start.elapsed() > timeout {
                return Err(Error::Timeout);
            }

            {
                let inbound_messages = self.inbound_messages.read().unwrap();

                for message in inbound_messages.iter() {
                    if (matcher)(message) {
                        return Ok(*message);
                    }
                }
            }

            thread::sleep(Duration::from_millis(10));
        }
    }

    fn receive_messages(&self) -> Result<(), Error> {
        let (tx, rx) = crossbeam_channel::unbounded();
        let endpoint = self.in_ep.ok_or(Error::EndpointNotInitialized)?;
        let handle = Arc::clone(&self.handle);

        thread::spawn(move || {
            let reader = HandleReader { endpoint, handle };
            let publisher = reader::Publisher::new(&reader, tx, 4096);
            publisher.run().expect("publisher run failed");
        });

        let inbound_messages = Arc::clone(&self.inbound_messages);
        let assigned = Arc::clone(&self.assigned);

        thread::spawn(move || loop {
            match rx.recv() {
                Ok(message) => {
                    println!("received: {}", message);

                    if let Message::BroadcastData(data) = message {
                        let mut assigned = assigned.lock().unwrap();
                        if let Some(device) = assigned.get_mut(&data.channel) {
                            if let Err(e) = device.process_data(data.data) {
                                println!("Error processing data: {:?}", e);
                            }
                        }
                    } else {
                        let mut inbound_messages = inbound_messages.write().unwrap();
                        inbound_messages.push(message)
                    }
                }
                Err(_) => {
                    println!("error receiving from publisher");
                    break;
                }
            }
        });

        Ok(())
    }

    pub fn read(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, Error> {
        let handle = self.handle.read().unwrap();
        let endpoint = self.in_ep.ok_or(Error::EndpointNotInitialized)?;
        match handle
            .as_ref()
            .expect("no handle")
            .read_bulk(endpoint.address, buf, timeout)
        {
            Ok(size) => Ok(size),
            Err(rusb::Error::Timeout) => Err(Error::Timeout),
            Err(e) => Err(e.into()),
        }
    }

    pub fn write_message(&self, message: message::Message, timeout: Duration) -> Result<(), Error> {
        self.write(message.encode().as_ref(), timeout)?;

        println!("sent: {}", message);
        Ok(())
    }

    pub fn write(&self, buf: &[u8], timeout: Duration) -> Result<usize, Error> {
        let handle = self.handle.read().unwrap();
        let endpoint = self.out_ep.ok_or(Error::EndpointNotInitialized)?;
        match handle
            .as_ref()
            .expect("no handle")
            .write_bulk(endpoint.address, buf, timeout)
        {
            Ok(size) => Ok(size),
            Err(rusb::Error::Timeout) => Err(Error::Timeout),
            Err(e) => Err(e.into()),
        }
    }

    fn find_device(&self) -> Result<rusb::Device<rusb::GlobalContext>, Error> {
        let devices = rusb::devices()?;

        for device in devices.iter() {
            let descriptor = device.device_descriptor()?;

            if descriptor.vendor_id() == self.vendor_id
                && descriptor.product_id() == self.product_id
            {
                return Ok(device);
            }
        }

        Err(Error::DeviceNotFound)
    }

    fn find_endpoints(&self) -> Result<(Endpoint, Endpoint), Error> {
        let device = self.device.clone().ok_or(Error::DeviceNotInitialized)?;

        let config = device.config_descriptor(0)?;

        let interfaces = config.interfaces();

        let mut in_endpoint = None;
        let mut out_endpoint = None;

        for interface in interfaces {
            for descriptor in interface.descriptors() {
                for endpoint in descriptor.endpoint_descriptors() {
                    if endpoint.usage_type() == rusb::UsageType::Data
                        && endpoint.transfer_type() == rusb::TransferType::Bulk
                    {
                        let result = Some(Endpoint {
                            interface: interface.number(),
                            address: endpoint.address(),
                        });

                        match endpoint.direction() {
                            rusb::Direction::In => in_endpoint = result,
                            rusb::Direction::Out => out_endpoint = result,
                        }
                    }
                }
            }
        }

        if let Some(in_ep) = in_endpoint {
            if let Some(out_ep) = out_endpoint {
                return Ok((in_ep, out_ep));
            }
        }

        Err(Error::EndpointNotFound)
    }
}

pub trait Reader {
    fn read(&self, buf: &mut [u8], timeout: Duration) -> Result<usize, crate::node::Error>;
}

pub struct HandleReader {
    handle: Arc<RwLock<Option<rusb::DeviceHandle<rusb::GlobalContext>>>>,
    endpoint: Endpoint,
}

impl Reader for HandleReader {
    fn read(&self, buf: &mut [u8], timeout: Duration) -> Result<usize, crate::node::Error> {
        let guard = self.handle.read().unwrap();
        let handle = guard.as_ref().ok_or(Error::HandleNotInitialized)?;
        Ok(handle.read_bulk(self.endpoint.address, buf, timeout)?)
    }
}

pub struct NodeBuilder {
    vendor_id: u16,
    product_id: u16,
    network_key: [u8; 8],
}

impl NodeBuilder {
    pub fn new(network_key: [u8; 8]) -> NodeBuilder {
        NodeBuilder {
            vendor_id: DYNASTREAM_INNOVATIONS_VID,
            product_id: DI_ANT_M_STICK,
            network_key,
        }
    }

    pub fn build(&self) -> Node {
        Node {
            capabilities: None,
            vendor_id: self.vendor_id,
            product_id: self.product_id,
            network_key: self.network_key,
            device: None,
            handle: Arc::new(RwLock::new(None)),
            in_ep: None,
            out_ep: None,
            inbound_messages: Arc::new(RwLock::new(vec![])),
            assigned: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

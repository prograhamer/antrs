mod capabilities;

use core::time::Duration;
use log::{error, trace};
use std::collections::{hash_map, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use crate::device;
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
    ExtendedMessagesNotSupported,
    ChannelDisconnected,
    ChannelInvalidState,
}

impl From<rusb::Error> for Error {
    fn from(value: rusb::Error) -> Self {
        match value {
            rusb::Error::Timeout => Error::Timeout,
            _ => Error::USBError(value),
        }
    }
}

impl From<crossbeam_channel::RecvTimeoutError> for Error {
    fn from(value: crossbeam_channel::RecvTimeoutError) -> Self {
        match value {
            crossbeam_channel::RecvTimeoutError::Timeout => Error::Timeout,
            crossbeam_channel::RecvTimeoutError::Disconnected => Error::ChannelDisconnected,
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

struct MessageNotifier {
    id: u64,
    matcher: Box<dyn Fn(Message) -> bool + Send>,
    sender: crossbeam_channel::Sender<Message>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChannelStatus {
    Assigned,
    Open,
    Closing,
    Closed,
}

struct ChannelAssignment {
    device: Option<Box<dyn device::DataProcessor + Send>>,
    status: ChannelStatus,
    events: Vec<MessageCode>,
}

/// Options to configure opened channels.
///
/// Note: if multiple channels are entering search mode, e.g. when opening multiple channels
/// with paired devices, the channels will enter search sequentially. E.g. opening two channels
/// both with search timeouts of 10 seconds, the first channel will close with search timeout
/// after 10 seconds, and the second after 20 seconds (10 seconds after the first closed and the
/// second entered search).
pub struct ChannelOptions {
    /// Timeout for low priority device search in 2.5 seconds increments, with special cases of
    /// 0 meaning no low priority search and 255 meaning no timeout. If not specified, the device
    /// default or previously set value will be used.
    ///
    /// If supported by the node, low priority search is performed before entering high priority
    /// search. Low priority search will not interrupt other open channels while searching.
    pub low_priority_search_timeout: Option<u8>,
    /// Timeout for device search specified in 2.5 second increments, with special cases of
    /// 0 meaning immediate timeout and 255 meaning no timeout. If not specified, the device
    /// default or previously set value will be used.
    pub search_timeout: Option<u8>,
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
    notifiers: Arc<Mutex<Vec<MessageNotifier>>>,
    assigned: Arc<RwLock<HashMap<u8, Mutex<ChannelAssignment>>>>,
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
        self.expect_channel_response_no_error_after(
            0,
            MessageID::SetNetworkKey,
            Duration::from_millis(1000),
            || self.write_message(set_network_key, Duration::from_millis(100)),
        )?;

        let request_capabilities = Message::RequestMessage(RequestMessageData {
            channel: 0,
            message_id: MessageID::Capabilities,
        });
        let capabilities = self.wait_for_message_after(
            Box::new(|message| matches!(message, Message::Capabilities(_))),
            Duration::from_millis(1000),
            || self.write_message(request_capabilities, Duration::from_millis(100)),
        )?;
        if let Message::Capabilities(data) = capabilities {
            self.capabilities = Some(data.into())
        }

        Ok(())
    }

    pub fn close(&mut self) -> Result<(), Error> {
        let assigned = Arc::clone(&self.assigned);
        let assigned = assigned.read().unwrap();

        for &channel in assigned.keys() {
            self.close_channel(channel)?;
        }

        let mut handle = self.handle.write().unwrap();
        if let Some(ref mut handle) = *handle {
            handle.reset()?;
        }

        Ok(())
    }

    pub fn close_channel(&mut self, channel: u8) -> Result<(), Error> {
        let assigned = self.assigned.read().unwrap();
        if let Some(assignment) = assigned.get(&channel) {
            let mut assignment = assignment.lock().unwrap();
            if assignment.status == ChannelStatus::Open {
                assignment.status = ChannelStatus::Closing;
                drop(assignment);

                self.wait_for_message_after(
                    Box::new(move |message| {
                        if let Message::ChannelResponseEvent(data) = message {
                            data.channel == channel
                                && data.message_id == MessageID::ChannelEvent
                                && data.message_code == MessageCode::EventChannelClosed
                        } else {
                            false
                        }
                    }),
                    Duration::from_secs(1),
                    || {
                        self.expect_channel_response_no_error_after(
                            channel,
                            MessageID::CloseChannel,
                            Duration::from_millis(1000),
                            || {
                                self.write_message(
                                    Message::CloseChannel(message::CloseChannelData { channel }),
                                    Duration::from_millis(100),
                                )?;
                                Ok(())
                            },
                        )
                    },
                )?;
            }
        }

        Ok(())
    }

    pub fn free_channel(&mut self, channel: u8) -> Result<(), Error> {
        let mut assigned = self.assigned.write().unwrap();

        let ok = if let Some(assignment) = assigned.get(&channel) {
            let assignment = assignment.lock().unwrap();
            assignment.status == ChannelStatus::Closed
        } else {
            false
        };

        if ok {
            assigned.remove(&channel);
            Ok(())
        } else {
            Err(Error::ChannelInvalidState)
        }
    }

    pub fn channel_status(&self, channel: u8) -> Option<(ChannelStatus, Vec<MessageCode>)> {
        let assigned = self.assigned.read().unwrap();
        if let Some(assignment) = assigned.get(&channel) {
            let assignment = assignment.lock().unwrap();
            return Some((assignment.status, assignment.events.clone()));
        }
        None
    }

    pub fn search(
        &mut self,
        options: Option<ChannelOptions>,
    ) -> Result<(u8, crossbeam_channel::Receiver<message::ChannelID>), Error> {
        let (search, receiver) = device::Search::new();

        let channel = self._assign_channel(Box::new(search))?;

        let enable_extended_messages =
            Message::EnableExtendedMessages(message::EnableExtendedMessagesData { enabled: 1 });
        self.expect_channel_response_no_error_after(
            channel,
            MessageID::EnableExtendedMessages,
            Duration::from_millis(100),
            || self.write_message(enable_extended_messages, Duration::from_millis(100)),
        )?;

        let assign_channel = Message::AssignChannel(message::AssignChannelData {
            channel,
            channel_type: message::ChannelType::Receive,
            network: 0,
            extended_assignment: message::ChannelExtendedAssignment::BACKGROUND_SCANNING,
        });
        self.expect_channel_response_no_error_after(
            channel,
            MessageID::AssignChannel,
            Duration::from_millis(100),
            || self.write_message(assign_channel, Duration::from_millis(100)),
        )?;

        let set_channel_id = Message::SetChannelID(message::SetChannelIDData {
            channel,
            device: 0,
            pairing: false,
            device_type: 0,
            transmission_type: 0,
        });
        self.expect_channel_response_no_error_after(
            channel,
            MessageID::SetChannelID,
            Duration::from_millis(100),
            || self.write_message(set_channel_id, Duration::from_secs(100)),
        )?;

        let set_channel_period = Message::SetChannelPeriod(message::SetChannelPeriodData {
            channel,
            period: 8070,
        });
        self.expect_channel_response_no_error_after(
            channel,
            MessageID::SetChannelPeriod,
            Duration::from_millis(100),
            || self.write_message(set_channel_period, Duration::from_millis(100)),
        )?;

        let set_channel_rf_freq =
            Message::SetChannelRFFrequency(message::SetChannelRFFrequencyData {
                channel,
                frequency: 57,
            });
        self.expect_channel_response_no_error_after(
            channel,
            MessageID::SetChannelRFFrequency,
            Duration::from_millis(100),
            || self.write_message(set_channel_rf_freq, Duration::from_millis(100)),
        )?;

        if let Some(options) = options {
            if let Some(timeout) = options.low_priority_search_timeout {
                let search_timeout = Message::SetChannelLowPrioritySearchTimeout(
                    message::SetChannelLowPrioritySearchTimeoutData { channel, timeout },
                );
                self.expect_channel_response_no_error_after(
                    channel,
                    MessageID::SetChannelLowPrioritySearchTimeout,
                    Duration::from_millis(100),
                    || self.write_message(search_timeout, Duration::from_millis(100)),
                )?;
            }

            if let Some(timeout) = options.search_timeout {
                let search_timeout =
                    Message::SetChannelSearchTimeout(message::SetChannelSearchTimeoutData {
                        channel,
                        timeout,
                    });
                self.expect_channel_response_no_error_after(
                    channel,
                    MessageID::SetChannelSearchTimeout,
                    Duration::from_millis(100),
                    || self.write_message(search_timeout, Duration::from_millis(100)),
                )?;
            }
        }

        let open_channel = Message::OpenChannel(message::OpenChannelData { channel });
        self.expect_channel_response_no_error_after(
            channel,
            MessageID::OpenChannel,
            Duration::from_millis(100),
            || self.write_message(open_channel, Duration::from_millis(100)),
        )?;

        Ok((channel, receiver))
    }

    fn _assign_channel(
        &mut self,
        processor: Box<dyn device::DataProcessor + Send>,
    ) -> Result<u8, Error> {
        let max_channels;

        if let Some(capabilities) = &self.capabilities {
            max_channels = capabilities.max_channels;
        } else {
            return Err(Error::CapabilitiesNotInitialized);
        }

        let mut assigned = self.assigned.write().unwrap();
        for i in 0..max_channels {
            if let hash_map::Entry::Vacant(e) = assigned.entry(i) {
                e.insert(Mutex::new(ChannelAssignment {
                    status: ChannelStatus::Assigned,
                    device: Some(processor),
                    events: Vec::new(),
                }));
                return Ok(i);
            }
        }

        Err(Error::NoAvailableChannel)
    }

    pub fn assign_channel(
        &mut self,
        device: Box<dyn device::Device + Send>,
        options: Option<ChannelOptions>,
    ) -> Result<u8, Error> {
        let channel = self._assign_channel(device.as_data_processor())?;

        let assign_channel = Message::AssignChannel(message::AssignChannelData {
            channel,
            channel_type: device.channel_type(),
            network: 0,
            extended_assignment: message::ChannelExtendedAssignment::empty(),
        });
        self.expect_channel_response_no_error_after(
            channel,
            MessageID::AssignChannel,
            Duration::from_millis(100),
            || self.write_message(assign_channel, Duration::from_millis(100)),
        )?;

        let pairing = device.pairing();
        let set_channel_id = Message::SetChannelID(message::SetChannelIDData {
            channel,
            device: pairing.device_id,
            pairing: false,
            device_type: device.device_type(),
            transmission_type: pairing.transmission_type,
        });

        self.expect_channel_response_no_error_after(
            channel,
            MessageID::SetChannelID,
            Duration::from_millis(100),
            || self.write_message(set_channel_id, Duration::from_secs(100)),
        )?;

        let set_channel_period = Message::SetChannelPeriod(message::SetChannelPeriodData {
            channel,
            period: device.channel_period(),
        });
        self.expect_channel_response_no_error_after(
            channel,
            MessageID::SetChannelPeriod,
            Duration::from_millis(100),
            || self.write_message(set_channel_period, Duration::from_millis(100)),
        )?;

        let set_channel_rf_freq =
            Message::SetChannelRFFrequency(message::SetChannelRFFrequencyData {
                channel,
                frequency: device.rf_frequency(),
            });
        self.expect_channel_response_no_error_after(
            channel,
            MessageID::SetChannelRFFrequency,
            Duration::from_millis(100),
            || self.write_message(set_channel_rf_freq, Duration::from_millis(100)),
        )?;

        if let Some(options) = options {
            if let Some(timeout) = options.low_priority_search_timeout {
                let search_timeout = Message::SetChannelLowPrioritySearchTimeout(
                    message::SetChannelLowPrioritySearchTimeoutData { channel, timeout },
                );
                self.expect_channel_response_no_error_after(
                    channel,
                    MessageID::SetChannelLowPrioritySearchTimeout,
                    Duration::from_millis(100),
                    || self.write_message(search_timeout, Duration::from_millis(100)),
                )?;
            }

            if let Some(timeout) = options.search_timeout {
                let search_timeout =
                    Message::SetChannelSearchTimeout(message::SetChannelSearchTimeoutData {
                        channel,
                        timeout,
                    });
                self.expect_channel_response_no_error_after(
                    channel,
                    MessageID::SetChannelSearchTimeout,
                    Duration::from_millis(100),
                    || self.write_message(search_timeout, Duration::from_millis(100)),
                )?;
            }
        }

        let open_channel = Message::OpenChannel(message::OpenChannelData { channel });
        self.expect_channel_response_no_error_after(
            channel,
            MessageID::OpenChannel,
            Duration::from_millis(100),
            || self.write_message(open_channel, Duration::from_millis(100)),
        )?;

        {
            let assigned = self.assigned.read().unwrap();
            let assignment = assigned
                .get(&channel)
                .expect("should contain new assignment");
            let mut assignment = assignment.lock().unwrap();
            assignment.status = ChannelStatus::Open;
        }

        Ok(channel)
    }

    fn expect_channel_response_no_error_after<T, F: FnOnce() -> Result<T, Error>>(
        &self,
        channel: u8,
        message_id: MessageID,
        timeout: Duration,
        after: F,
    ) -> Result<(), Error> {
        let message = self.wait_for_message_after(
            Box::new(move |message| {
                if let Message::ChannelResponseEvent(data) = message {
                    data.channel == channel && data.message_id == message_id
                } else {
                    false
                }
            }),
            timeout,
            after,
        )?;

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

    fn wait_for_message_after<T, F: FnOnce() -> Result<T, Error>>(
        &self,
        matcher: Box<dyn Fn(Message) -> bool + Send>,
        timeout: Duration,
        after: F,
    ) -> Result<Message, Error> {
        let receiver = self.notify(matcher);
        (after)()?;
        Ok(receiver.recv_timeout(timeout)?)
    }

    fn notify(
        &self,
        matcher: Box<dyn Fn(Message) -> bool + Send>,
    ) -> crossbeam_channel::Receiver<Message> {
        static ID_SEQ: AtomicU64 = AtomicU64::new(0);

        let id = ID_SEQ.fetch_add(1, Ordering::Relaxed);

        let (sender, receiver) = crossbeam_channel::bounded(1);
        let mut notifiers = self.notifiers.lock().unwrap();
        notifiers.push(MessageNotifier {
            id,
            matcher,
            sender: sender.clone(),
        });
        receiver
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

        let assigned = Arc::clone(&self.assigned);
        let notifiers = Arc::clone(&self.notifiers);

        thread::spawn(move || {
            let send_notifications = move |message| {
                let mut notifiers = notifiers.lock().unwrap();
                let mut to_delete = vec![];
                for notifier in notifiers.iter() {
                    if (notifier.matcher)(message) {
                        to_delete.push(notifier.id);
                        if let Err(e) = notifier.sender.try_send(message) {
                            error!("failed to notify of message: {:?}: {}", message, e)
                        }
                    }
                }

                notifiers.retain(|n| !to_delete.contains(&n.id));
            };

            loop {
                match rx.recv() {
                    Ok(message) => {
                        trace!("received: {}", message);

                        match message {
                            Message::BroadcastData(data) | Message::AcknowledgedData(data) => {
                                let assigned = assigned.read().unwrap();
                                if let Some(assignment) = assigned.get(&data.channel) {
                                    let mut assignment = assignment.lock().unwrap();
                                    if let Some(ref mut device) = assignment.device {
                                        if let Err(e) = device.process_data(data) {
                                            error!("Error processing data: {:?}", e);
                                        }
                                    }
                                }
                            }
                            Message::ChannelResponseEvent(data) => {
                                if data.message_id == MessageID::ChannelEvent {
                                    let assigned = assigned.read().unwrap();
                                    if let Some(assignment) = assigned.get(&data.channel) {
                                        let mut assignment = assignment.lock().unwrap();
                                        if data.message_code == MessageCode::EventChannelClosed {
                                            assignment.status = ChannelStatus::Closed;
                                            assignment.device = None;
                                        }
                                        assignment.events.push(data.message_code);
                                    }
                                }
                                send_notifications(message);
                            }
                            _ => {
                                send_notifications(message);
                            }
                        }
                    }
                    Err(_) => {
                        error!("error receiving from publisher");
                        break;
                    }
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

        trace!("sent: {}", message);
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
            notifiers: Arc::new(Mutex::new(vec![])),
            assigned: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

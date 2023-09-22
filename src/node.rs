use core::time::Duration;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use crate::device::Device;
use crate::message;

#[derive(Debug)]
pub enum Error {
    DeviceNotFound,
    DeviceNotInitialized,
    EndpointNotInitialized,
    HandleNotInitialized,
    EndpointNotFound,
    Timeout,
    USBError(rusb::Error),
}

impl From<rusb::Error> for Error {
    fn from(value: rusb::Error) -> Self {
        Error::USBError(value)
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
    vendor_id: u16,
    product_id: u16,
    device: Option<rusb::Device<rusb::GlobalContext>>,
    handle: Arc<Mutex<Option<rusb::DeviceHandle<rusb::GlobalContext>>>>,
    in_ep: Option<Endpoint>,
    out_ep: Option<Endpoint>,
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

        let mut h = self.handle.lock().unwrap();
        *h = Some(handle);

        Ok(())
    }

    pub fn assign_channel(&mut self, device: &dyn Device) {
        let channel_type = device.channel_type();
        let rf_freq = device.rf_frequency();
        let pairing = device.pairing();
        let device_type = device.device_type();

        println!(
            "channel_type={}, rf_freq={}, pairing={}, device_type={}",
            channel_type, rf_freq, pairing, device_type
        );
    }

    pub fn receive_messages(&self) -> Result<mpsc::Receiver<message::Message>, Error> {
        let (tx, rx) = mpsc::channel::<message::Message>();
        let handle_mutex = Arc::clone(&self.handle);
        let endpoint_id = self.in_ep.ok_or(Error::EndpointNotInitialized)?;

        thread::Builder::new()
            .name(String::from("node message read loop"))
            .spawn(move || {
                let buf = &mut [0u8; 1024];
                let mut write_index = 0usize;
                let mut read_index = 0usize;

                loop {
                    let read_size;

                    {
                        let mut guard = handle_mutex.lock().unwrap();
                        let handle = guard.as_mut().expect("no handle");

                        read_size = match handle.read_bulk(
                            endpoint_id.address,
                            &mut buf[write_index..],
                            Duration::new(0, 100_000_000),
                        ) {
                            Ok(size) => size,
                            Err(e) => {
                                if e != rusb::Error::Timeout {
                                    panic!("read from endpoint: {}", e)
                                } else {
                                    0
                                }
                            }
                        };
                        write_index += read_size;
                    }

                    if read_size > 0 {
                        let mut discard_count = 0usize;
                        while buf[read_index] != message::SYNC && read_index < write_index {
                            read_index += 1;
                            discard_count += 1;
                        }

                        if discard_count > 0 {
                            println!("discarded {} bytes!", discard_count);
                        }

                        while read_index < write_index - 5 {
                            let msg = match message::Message::decode(&buf[read_index..]) {
                                Ok(msg) => msg,
                                Err(e) => panic!("decoding message: {}", e),
                            };

                            read_index += msg.encoded_len();

                            tx.send(msg).expect("send should succeed");
                        }

                        if read_index == write_index {
                            read_index = 0;
                            write_index = 0;
                        } else if read_index > 0 {
                            let offset = write_index - read_index;

                            for i in 0..offset {
                                buf[i] = buf[read_index + i];
                            }

                            read_index = 0;
                            write_index = offset;
                        }
                    }

                    thread::sleep(Duration::new(0, 10_000_000));
                }
            })
            .unwrap();

        Ok(rx)
    }

    pub fn read(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, Error> {
        let mut handle = self.handle.lock().unwrap();
        let endpoint = self.in_ep.ok_or(Error::EndpointNotInitialized)?;
        match handle
            .as_mut()
            .expect("no handle")
            .read_bulk(endpoint.address, buf, timeout)
        {
            Ok(size) => Ok(size),
            Err(rusb::Error::Timeout) => Err(Error::Timeout),
            Err(e) => Err(e.into()),
        }
    }

    pub fn write_message(
        &mut self,
        message: message::Message,
        timeout: Duration,
    ) -> Result<(), Error> {
        self.write(message.encode().as_ref(), timeout)?;

        println!("sent: {}", message);
        Ok(())
    }

    pub fn write(&mut self, buf: &[u8], timeout: Duration) -> Result<usize, Error> {
        let mut handle = self.handle.lock().unwrap();
        let endpoint = self.out_ep.ok_or(Error::EndpointNotInitialized)?;
        match handle
            .as_mut()
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

pub struct NodeBuilder {
    vendor_id: u16,
    product_id: u16,
}

impl Default for NodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeBuilder {
    pub fn new() -> NodeBuilder {
        NodeBuilder {
            vendor_id: DYNASTREAM_INNOVATIONS_VID,
            product_id: DI_ANT_M_STICK,
        }
    }

    pub fn build(&self) -> Node {
        Node {
            vendor_id: self.vendor_id,
            product_id: self.product_id,
            device: None,
            handle: Arc::new(Mutex::new(None)),
            in_ep: None,
            out_ep: None,
        }
    }
}

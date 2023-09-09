use core::time::Duration;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use crate::message;

#[derive(Debug)]
pub enum Error {
    DeviceNotFound,
    DeviceNotInitialized,
    HandleNotInitialized,
    EndpointNotFound,
    Timeout,
    USBError(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

const DYNASTREAM_INNOVATIONS_VID: u16 = 0xfcf;
const DI_ANT_M_STICK: u16 = 0x1009;

pub struct Node {
    vendor_id: u16,
    product_id: u16,
    device: Option<rusb::Device<rusb::GlobalContext>>,
    handle: Arc<Mutex<Option<rusb::DeviceHandle<rusb::GlobalContext>>>>,
    in_ep_id: u8,
    out_ep_id: u8,
}

impl Node {
    pub fn open(&mut self) -> Result<(), Error> {
        self.device = Some(self.find_device()?);

        let (in_ep_id, out_ep_id) = self.find_endpoints()?;
        self.in_ep_id = in_ep_id;
        self.out_ep_id = out_ep_id;

        match self
            .device
            .clone()
            .ok_or(Error::DeviceNotInitialized)?
            .open()
        {
            Ok(handle) => {
                let mut h = self.handle.lock().unwrap();
                *h = Some(handle);
            }
            Err(e) => return Err(Error::USBError(e.to_string())),
        };

        self.detach_kernel_drivers()?;

        Ok(())
    }

    pub fn receive_messages(&self) -> mpsc::Receiver<message::Message> {
        let (tx, rx) = mpsc::channel::<message::Message>();
        let handle_mutex = Arc::clone(&self.handle);
        let endpoint_id = self.in_ep_id;

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
                            endpoint_id,
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

        rx
    }

    pub fn read(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, Error> {
        let mut handle = self.handle.lock().unwrap();
        match handle
            .as_mut()
            .expect("no handle")
            .read_bulk(self.in_ep_id, buf, timeout)
        {
            Ok(size) => Ok(size),
            Err(rusb::Error::Timeout) => Err(Error::Timeout),
            Err(e) => Err(Error::USBError(e.to_string())),
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
        match handle
            .as_mut()
            .expect("no handle")
            .write_bulk(self.out_ep_id, buf, timeout)
        {
            Ok(size) => Ok(size),
            Err(rusb::Error::Timeout) => Err(Error::Timeout),
            Err(e) => Err(Error::USBError(e.to_string())),
        }
    }

    fn find_device(&self) -> Result<rusb::Device<rusb::GlobalContext>, Error> {
        let devices = match rusb::devices() {
            Ok(_d) => _d,
            Err(e) => return Err(Error::USBError(e.to_string())),
        };

        for device in devices.iter() {
            let descriptor = match device.device_descriptor() {
                Ok(_d) => _d,
                Err(e) => {
                    return Err(Error::USBError(format!(
                        "getting descriptor for bus {} / address {}: {}",
                        device.bus_number(),
                        device.address(),
                        e
                    )))
                }
            };

            if descriptor.vendor_id() == self.vendor_id
                && descriptor.product_id() == self.product_id
            {
                return Ok(device);
            }
        }

        Err(Error::DeviceNotFound)
    }

    fn find_endpoints(&self) -> Result<(u8, u8), Error> {
        let device = self.device.clone().ok_or(Error::DeviceNotInitialized)?;

        let config = match device.config_descriptor(0) {
            Ok(_c) => _c,
            Err(e) => return Err(Error::USBError(e.to_string())),
        };

        let interfaces = config.interfaces();

        let mut in_endpoint = None;
        let mut out_endpoint = None;

        for interface in interfaces {
            for descriptor in interface.descriptors() {
                for endpoint in descriptor.endpoint_descriptors() {
                    if endpoint.usage_type() == rusb::UsageType::Data
                        && endpoint.transfer_type() == rusb::TransferType::Bulk
                    {
                        match endpoint.direction() {
                            rusb::Direction::In => in_endpoint = Some(endpoint),
                            rusb::Direction::Out => out_endpoint = Some(endpoint),
                        }
                    }
                }
            }
        }

        if let Some(in_ep) = in_endpoint {
            if let Some(out_ep) = out_endpoint {
                return Ok((in_ep.address(), out_ep.address()));
            }
        }

        Err(Error::EndpointNotFound)
    }

    fn detach_kernel_drivers(&mut self) -> Result<(), Error> {
        let device = self.device.clone().ok_or(Error::DeviceNotInitialized)?;
        let mut handle_mutex = self.handle.lock().unwrap();
        let handle = handle_mutex.as_mut().ok_or(Error::HandleNotInitialized)?;

        let config = match device.config_descriptor(0) {
            Ok(_c) => _c,
            Err(e) => return Err(Error::USBError(e.to_string())),
        };

        for interface in config.interfaces() {
            match handle.kernel_driver_active(interface.number()) {
                Ok(active) => {
                    if active {
                        match handle.detach_kernel_driver(0) {
                            Err(e) => return Err(Error::USBError(e.to_string())),
                            Ok(e) => e,
                        }
                    }
                }
                Err(e) => {
                    if e != rusb::Error::NotSupported {
                        return Err(Error::USBError(e.to_string()));
                    }
                }
            };
        }

        Ok(())
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
            in_ep_id: 0,
            out_ep_id: 0,
        }
    }
}

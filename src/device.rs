use std::collections::HashSet;

use crate::message;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Error {
    InvalidValue,
    SendError,
}

impl<T> From<crossbeam_channel::TrySendError<T>> for Error {
    fn from(_error: crossbeam_channel::TrySendError<T>) -> Self {
        Error::SendError
    }
}

pub trait Device: DataProcessor {
    fn channel_type(&self) -> message::ChannelType;
    fn device_type(&self) -> u8;
    fn rf_frequency(&self) -> u8;

    fn channel_period(&self) -> u16;
    fn set_channel_period(&mut self, period: u16) -> Result<(), Error>;
    fn pairing(&self) -> DevicePairing;

    fn as_data_processor(&self) -> Box<dyn DataProcessor + Send>;
}

pub trait DataProcessor {
    fn process_data(&mut self, data: message::BroadcastDataData) -> Result<(), Error>;
}

#[derive(Clone, Copy, Debug)]
pub struct DevicePairing {
    pub device_id: u16,
    pub transmission_type: u8,
}

impl std::fmt::Display for DevicePairing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub struct Search {
    sender: crossbeam_channel::Sender<message::BroadcastChannelID>,
    found: HashSet<message::BroadcastChannelID>,
}

impl Search {
    pub fn new() -> (
        Search,
        crossbeam_channel::Receiver<message::BroadcastChannelID>,
    ) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let search = Search {
            sender,
            found: HashSet::new(),
        };
        (search, receiver)
    }
}

impl DataProcessor for Search {
    fn process_data(&mut self, data: message::BroadcastDataData) -> Result<(), Error> {
        if let Some(id) = data.channel_id {
            if !self.found.contains(&id) {
                self.sender.try_send(id)?;
                self.found.insert(id);
            }
        }
        Ok(())
    }
}

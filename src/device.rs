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

pub trait Device {
    fn channel_type(&self) -> u8;
    fn device_type(&self) -> u8;
    fn rf_frequency(&self) -> u8;

    fn channel_period(&self) -> u16;
    fn set_channel_period(&mut self, period: u16) -> Result<(), Error>;
    fn pairing(&self) -> DevicePairing;
    fn process_data(&mut self, data: [u8; 8]) -> Result<(), Error>;
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

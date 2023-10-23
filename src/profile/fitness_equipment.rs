use num_enum::TryFromPrimitive;

use crate::device::{DataProcessor, Device, DevicePairing, Error};
use crate::{bytes, message};

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, TryFromPrimitive)]
pub enum EquipmentType {
    Treadmill = 19,
    Elliptical = 20,
    Rower = 22,
    Climber = 23,
    NordicSkier = 24,
    StationaryBike = 25,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, TryFromPrimitive)]
pub enum EquipmentState {
    Asleep = 1,
    Ready = 2,
    InUse = 3,
    Finished = 4,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum HRDataSource {
    HandContactSensors = 3,
    EM5KHzMonitor = 2,
    ANTMonitor = 1,
    Invalid = 0,
}

impl TryFrom<u8> for HRDataSource {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value & 0x03 {
            0 => Ok(HRDataSource::Invalid),
            1 => Ok(HRDataSource::ANTMonitor),
            2 => Ok(HRDataSource::EM5KHzMonitor),
            3 => Ok(HRDataSource::HandContactSensors),
            _ => Err(Error::InvalidValue),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TargetPowerStatus {
    Undetermined,
    SpeedTooLow,
    SpeedTooHigh,
    Ok,
}

impl TryFrom<u8> for TargetPowerStatus {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value & 0x03 {
            0 => Ok(TargetPowerStatus::Ok),
            1 => Ok(TargetPowerStatus::SpeedTooLow),
            2 => Ok(TargetPowerStatus::SpeedTooHigh),
            3 => Ok(TargetPowerStatus::Undetermined),
            _ => Err(Error::InvalidValue),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FitnessEquipment {
    pairing: DevicePairing,
    sender: crossbeam_channel::Sender<FitnessEquipmentData>,
}

pub fn new_paired(
    pairing: DevicePairing,
) -> (
    FitnessEquipment,
    crossbeam_channel::Receiver<FitnessEquipmentData>,
) {
    let (sender, receiver) = crossbeam_channel::unbounded();
    (FitnessEquipment { pairing, sender }, receiver)
}

impl Device for FitnessEquipment {
    fn channel_type(&self) -> message::ChannelType {
        message::ChannelType::Receive
    }

    fn device_type(&self) -> u8 {
        17
    }

    fn rf_frequency(&self) -> u8 {
        57
    }

    fn channel_period(&self) -> u16 {
        8192
    }

    fn pairing(&self) -> DevicePairing {
        self.pairing
    }

    fn as_data_processor(&self) -> Box<dyn DataProcessor + Send> {
        Box::new(self.clone())
    }
}

impl DataProcessor for FitnessEquipment {
    fn process_data(&mut self, data: message::BroadcastDataData) -> Result<(), Error> {
        if let Some(data) = data.data {
            let state = ((data[7] >> 4) & 0x07)
                .try_into()
                .or(Err(Error::InvalidValue))?;
            let lap_toggle = bytes::test_bit(data[7], 7);

            let page = match data[0] {
                16 => FitnessEquipmentData::Page16(Page16Data {
                    equipment_type: (data[1] & 0x1f).try_into().or(Err(Error::InvalidValue))?,
                    elapsed_time: data[2],
                    distance_traveled: data[3],
                    speed: match bytes::u8_to_u16(data[4], data[5]) {
                        0xffff => None,
                        speed => Some(speed),
                    },
                    heart_rate: match data[6] {
                        0xff => None,
                        hr => Some(hr),
                    },
                    hr_data_source: (data[7] & 0x03).try_into()?,
                    distance_traveled_enabled: bytes::test_bit(data[7], 2),
                    virtual_speed_flag: bytes::test_bit(data[7], 3),

                    state,
                    lap_toggle,
                }),
                25 => {
                    let instantaneous_power = match bytes::u8_to_u16(data[5], data[6] & 0x0f) {
                        0xfff => None,
                        power => Some(power),
                    };
                    FitnessEquipmentData::Page25(Page25Data {
                        update_event_count: data[1],
                        cadence: match data[2] {
                            0xff => None,
                            cadence => Some(cadence),
                        },
                        accumulated_power: if instantaneous_power.is_none() {
                            None
                        } else {
                            Some(bytes::u8_to_u16(data[3], data[4]))
                        },
                        instantaneous_power,
                        power_calibration_required: bytes::test_bit(data[6], 4),
                        resistance_calibration_required: bytes::test_bit(data[6], 5),
                        user_configuration_required: bytes::test_bit(data[6], 6),
                        target_power_status: (data[7] & 0x03).try_into()?,

                        state,
                        lap_toggle,
                    })
                }
                26 => FitnessEquipmentData::Page26(Page26Data {
                    update_event_count: data[1],
                    wheel_revolutions: data[2],
                    wheel_period: bytes::u8_to_u16(data[3], data[4]),
                    accumulated_torque: bytes::u8_to_u16(data[5], data[6]),

                    state,
                    lap_toggle,
                }),
                _ => return Ok(()),
            };

            self.sender.try_send(page)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Page16Data {
    pub equipment_type: EquipmentType,
    /// measured in 250ms ticks, wraparound at 64s
    pub elapsed_time: u8,
    /// measured in metres, wraparound at 256m
    pub distance_traveled: u8,
    /// measured in mm/s, max 65.534m/s
    pub speed: Option<u16>,
    pub heart_rate: Option<u8>,
    pub hr_data_source: HRDataSource,
    pub distance_traveled_enabled: bool,
    pub virtual_speed_flag: bool,

    // common fields
    pub state: EquipmentState,
    pub lap_toggle: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Page25Data {
    pub update_event_count: u8,
    pub cadence: Option<u8>,
    pub accumulated_power: Option<u16>,
    pub instantaneous_power: Option<u16>,
    pub power_calibration_required: bool,
    pub resistance_calibration_required: bool,
    pub user_configuration_required: bool,
    pub target_power_status: TargetPowerStatus,

    // common fields
    pub state: EquipmentState,
    pub lap_toggle: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Page26Data {
    pub update_event_count: u8,
    pub wheel_revolutions: u8,
    /// measured in 1/2048s, wraps around at 32s
    pub wheel_period: u16,
    /// measured in 1/32Nm, max 2048Nm
    pub accumulated_torque: u16,

    // common fields
    pub state: EquipmentState,
    pub lap_toggle: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FitnessEquipmentData {
    Page16(Page16Data),
    Page25(Page25Data),
    Page26(Page26Data),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitnessEquipmentDataOld {
    page: Option<u8>,

    // page 16 fields
    pub equipment_type: Option<EquipmentType>,
    /// measured in 250ms ticks, wraparound at 64s
    pub elapsed_time: Option<u8>,
    /// measured in metres, wraparound at 256m
    pub distance_traveled: Option<u8>,
    /// measured in mm/s, max 65.534m/s
    pub speed: Option<u16>,
    pub heart_rate: Option<u8>,
    pub hr_data_source: Option<HRDataSource>,
    pub distance_traveled_enabled: Option<bool>,
    pub virtual_speed_flag: Option<bool>,

    // page 25 fields
    pub p25_update_event_count: Option<u8>,
    pub cadence: Option<u8>,
    pub accumulated_power: Option<u16>,
    pub instantaneous_power: Option<u16>,
    pub power_calibration_required: Option<bool>,
    pub resistance_calibration_required: Option<bool>,
    pub user_configuration_required: Option<bool>,
    pub target_power_status: Option<TargetPowerStatus>,

    // common fields
    pub state: Option<EquipmentState>,
    pub lap_toggle: Option<bool>,
}

#[cfg(test)]
mod test {
    use super::{
        new_paired, EquipmentState, EquipmentType, FitnessEquipmentData, HRDataSource, Page16Data,
        Page25Data, Page26Data, TargetPowerStatus,
    };
    use crate::device::{DataProcessor, DevicePairing};
    use crate::message;

    const PAGE_16_TEST: message::BroadcastDataData = message::BroadcastDataData {
        channel: 0,
        data: Some([16, 25, 72, 150, 13, 20, 255, 36]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    };

    const PAGE_25_TEST: message::BroadcastDataData = message::BroadcastDataData {
        channel: 0,
        data: Some([25, 244, 87, 99, 32, 2, 97, 32]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    };

    const PAGE_26_TEST: message::BroadcastDataData = message::BroadcastDataData {
        channel: 0,
        data: Some([26, 247, 209, 140, 255, 239, 191, 32]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    };

    #[test]
    fn it_processes_page_16() {
        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 1,
        });
        assert_eq!(fe.process_data(PAGE_16_TEST), Ok(()));
        let data = receiver.try_recv().unwrap();
        assert_eq!(
            data,
            FitnessEquipmentData::Page16(Page16Data {
                equipment_type: EquipmentType::StationaryBike,
                elapsed_time: 72,
                distance_traveled: 150,
                speed: Some(5133),
                heart_rate: None,
                hr_data_source: HRDataSource::Invalid,
                distance_traveled_enabled: true,
                virtual_speed_flag: false,

                // common fields
                state: EquipmentState::Ready,
                lap_toggle: false,
            })
        );
    }

    #[test]
    fn it_processes_page_25() {
        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 0,
        });
        assert_eq!(fe.process_data(PAGE_25_TEST), Ok(()));
        let data = receiver.try_recv().unwrap();
        assert_eq!(
            data,
            FitnessEquipmentData::Page25(Page25Data {
                update_event_count: 244,
                cadence: Some(87),
                accumulated_power: Some(8291),
                instantaneous_power: Some(258),
                power_calibration_required: false,
                resistance_calibration_required: true,
                user_configuration_required: true,
                target_power_status: TargetPowerStatus::Ok,

                state: EquipmentState::Ready,
                lap_toggle: false,
            })
        );
    }

    #[test]
    fn it_processes_page_26() {
        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 0,
        });
        assert_eq!(fe.process_data(PAGE_26_TEST), Ok(()));
        let data = receiver.try_recv().unwrap();
        assert_eq!(
            data,
            FitnessEquipmentData::Page26(Page26Data {
                update_event_count: 247,
                wheel_revolutions: 209,
                wheel_period: 65420,
                accumulated_torque: 49135,

                state: EquipmentState::Ready,
                lap_toggle: false
            })
        );
    }
}

use num_enum::TryFromPrimitive;

use crate::device::{DataProcessor, Device, DevicePairing, Error};
use crate::message;
use log::warn;

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

pub fn target_power_message(channel: u8, power: u16) -> message::Message {
    let [power_lsb, power_msb] = power.to_le_bytes();

    message::Message::AcknowledgedData(message::DataPayload {
        channel,
        data: Some([0x31, 0xff, 0xff, 0xff, 0xff, 0xff, power_lsb, power_msb]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    })
}

pub fn wind_resistance_message(
    channel: u8,
    wind_resistance_coefficient: u8,
    wind_speed: u8,
    drafting_factor: u8,
) -> message::Message {
    message::Message::AcknowledgedData(message::DataPayload {
        channel,
        data: Some([
            0x32,
            0xff,
            0xff,
            0xff,
            0xff,
            wind_resistance_coefficient,
            wind_speed,
            drafting_factor,
        ]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    })
}

pub fn user_configuration_message(
    channel: u8,
    user_weight: u16,
    bike_weight: u16,
    wheel_diameter: u16,
) -> message::Message {
    let [user_weight_lsb, user_weight_msb] = user_weight.to_le_bytes();
    let wheel_diameter_dm = (wheel_diameter / 10).try_into().unwrap();
    let wheel_offset: u8 = (wheel_diameter % 10).try_into().unwrap();
    let [bike_weight_lsb, bike_weight_msb] = (bike_weight & 0x0fff).to_le_bytes();

    message::Message::AcknowledgedData(message::DataPayload {
        channel,
        data: Some([
            0x37,
            user_weight_lsb,
            user_weight_msb,
            0xff,
            bike_weight_lsb | (wheel_offset << 4),
            bike_weight_msb,
            wheel_diameter_dm,
            0,
        ]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    })
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
    fn process_data(&mut self, data: message::DataPayload) -> Result<(), Error> {
        if let Some(data) = data.data {
            let page = match data[0] {
                16 => FitnessEquipmentData::General(GeneralData {
                    equipment_type: (data[1] & 0x1f).try_into().or(Err(Error::InvalidValue))?,
                    elapsed_time: data[2],
                    distance_traveled: data[3],
                    speed: match u16::from_le_bytes([data[4], data[5]]) {
                        0xffff => None,
                        speed => Some(speed),
                    },
                    heart_rate: match data[6] {
                        0xff => None,
                        hr => Some(hr),
                    },
                    hr_data_source: (data[7] & 0x03).try_into()?,
                    distance_traveled_enabled: data[7] & (1 << 2) != 0,
                    virtual_speed_flag: data[7] & (1 << 3) != 0,

                    state: ((data[7] >> 4) & 0x07).try_into()?,
                    lap_toggle: data[7] & (1 << 7) != 0,
                }),
                25 => {
                    let instantaneous_power = match u16::from_le_bytes([data[5], data[6] & 0x0f]) {
                        0xfff => None,
                        power => Some(power),
                    };
                    FitnessEquipmentData::StationaryBike(StationaryBikeData {
                        update_event_count: data[1],
                        cadence: match data[2] {
                            0xff => None,
                            cadence => Some(cadence),
                        },
                        accumulated_power: if instantaneous_power.is_none() {
                            None
                        } else {
                            Some(u16::from_le_bytes([data[3], data[4]]))
                        },
                        instantaneous_power,
                        power_calibration_required: data[6] & 1 << 4 != 0,
                        resistance_calibration_required: data[6] & (1 << 5) != 0,
                        user_configuration_required: data[6] & (1 << 6) != 0,
                        target_power_status: (data[7] & 0x03).try_into()?,

                        state: ((data[7] >> 4) & 0x07).try_into()?,
                        lap_toggle: data[7] & (1 << 7) != 0,
                    })
                }
                26 => FitnessEquipmentData::StationaryBikeTorque(TorqueData {
                    update_event_count: data[1],
                    wheel_revolutions: data[2],
                    wheel_period: u16::from_le_bytes([data[3], data[4]]),
                    accumulated_torque: u16::from_le_bytes([data[5], data[6]]),

                    state: ((data[7] >> 4) & 0x07).try_into()?,
                    lap_toggle: data[7] & (1 << 7) != 0,
                }),
                54 => {
                    let maximum_resistance = match u16::from_le_bytes([data[5], data[6]]) {
                        0xffff => None,
                        value => Some(value),
                    };

                    FitnessEquipmentData::Capabilities(CapabilitiesData {
                        maximum_resistance,
                        basic_resistance: data[7] & (1 << 0) != 0,
                        target_power: data[7] & (1 << 1) != 0,
                        simulation: data[7] & (1 << 2) != 0,
                    })
                }
                _ => {
                    if let Some(common_data) = message::common::decode(data) {
                        match common_data {
                            message::common::DataPage::CommandStatus {
                                command_id,
                                sequence_no,
                                command_status,
                                response_data,
                            } => {
                                let mut command_status_data = CommandStatusData {
                                    command_id,
                                    sequence_no,
                                    command_status,
                                    total_resistance: None,
                                    target_power: None,
                                    wind_resistance_coefficient: None,
                                    wind_speed: None,
                                    drafting_factor: None,
                                    grade: None,
                                    rolling_resistance_coefficient: None,
                                };

                                if command_id == 49 {
                                    command_status_data.target_power = Some(u16::from_le_bytes([
                                        response_data[2],
                                        response_data[3],
                                    ]));
                                } else if command_id == 50 {
                                    command_status_data.wind_resistance_coefficient =
                                        Some(response_data[1]);
                                    command_status_data.wind_speed = Some(response_data[2]);
                                    command_status_data.drafting_factor = Some(response_data[3]);
                                }
                                FitnessEquipmentData::CommandStatus(command_status_data)
                            }
                            v => FitnessEquipmentData::Common(v),
                        }
                    } else {
                        warn!("received unhandled data page: {:?}", data);
                        return Ok(());
                    }
                }
            };

            self.sender.try_send(page)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeneralData {
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
pub struct StationaryBikeData {
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
pub struct TorqueData {
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
pub struct CommandStatusData {
    pub command_id: u8,
    pub sequence_no: u8,
    pub command_status: message::CommandStatus,
    pub total_resistance: Option<u8>,
    pub target_power: Option<u16>,
    pub wind_resistance_coefficient: Option<u8>,
    pub wind_speed: Option<u8>,
    pub drafting_factor: Option<u8>,
    pub grade: Option<u16>,
    pub rolling_resistance_coefficient: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CapabilitiesData {
    pub maximum_resistance: Option<u16>,
    pub basic_resistance: bool,
    pub target_power: bool,
    pub simulation: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FitnessEquipmentData {
    General(GeneralData),
    StationaryBike(StationaryBikeData),
    StationaryBikeTorque(TorqueData),
    Capabilities(CapabilitiesData),
    CommandStatus(CommandStatusData),
    Common(message::common::DataPage),
}

#[cfg(test)]
mod test {
    use super::{
        new_paired, EquipmentState, EquipmentType, FitnessEquipmentData, GeneralData, HRDataSource,
        StationaryBikeData, TargetPowerStatus, TorqueData,
    };
    use crate::device::{DataProcessor, DevicePairing};
    use crate::message::{self, CommandStatus};
    use crate::profile::fitness_equipment::{CapabilitiesData, CommandStatusData};

    #[test]
    fn it_processes_page_16() {
        let payload = message::DataPayload {
            channel: 0,
            data: Some([16, 25, 72, 150, 13, 20, 255, 36]),
            channel_id: None,
            rssi: None,
            rx_timestamp: None,
        };

        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 1,
        });
        assert_eq!(fe.process_data(payload), Ok(()));
        let data = receiver.try_recv().unwrap();
        assert_eq!(
            data,
            FitnessEquipmentData::General(GeneralData {
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
        let payload = message::DataPayload {
            channel: 0,
            data: Some([25, 244, 87, 99, 32, 2, 97, 32]),
            channel_id: None,
            rssi: None,
            rx_timestamp: None,
        };

        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 0,
        });
        assert_eq!(fe.process_data(payload), Ok(()));
        let data = receiver.try_recv().unwrap();
        assert_eq!(
            data,
            FitnessEquipmentData::StationaryBike(StationaryBikeData {
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
        let payload = message::DataPayload {
            channel: 0,
            data: Some([26, 247, 209, 140, 255, 239, 191, 32]),
            channel_id: None,
            rssi: None,
            rx_timestamp: None,
        };

        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 0,
        });
        assert_eq!(fe.process_data(payload), Ok(()));
        let data = receiver.try_recv().unwrap();
        assert_eq!(
            data,
            FitnessEquipmentData::StationaryBikeTorque(TorqueData {
                update_event_count: 247,
                wheel_revolutions: 209,
                wheel_period: 65420,
                accumulated_torque: 49135,

                state: EquipmentState::Ready,
                lap_toggle: false
            })
        );
    }

    #[test]
    fn it_processes_page_54() {
        let payload = message::DataPayload {
            channel: 0,
            data: Some([54, 0xff, 0xff, 0xff, 0xff, 0x10, 0x40, 0x03]),
            channel_id: None,
            rssi: None,
            rx_timestamp: None,
        };

        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 0,
        });
        assert_eq!(fe.process_data(payload), Ok(()));
        let data = receiver.try_recv().unwrap();
        assert_eq!(
            data,
            FitnessEquipmentData::Capabilities(CapabilitiesData {
                maximum_resistance: Some(0x4010),
                basic_resistance: true,
                target_power: true,
                simulation: false,
            })
        );
    }

    #[test]
    fn it_processes_page_54_no_resistance() {
        let payload = message::DataPayload {
            channel: 0,
            data: Some([54, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x06]),
            channel_id: None,
            rssi: None,
            rx_timestamp: None,
        };

        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 0,
        });
        assert_eq!(fe.process_data(payload), Ok(()));
        let data = receiver.try_recv().unwrap();
        assert_eq!(
            data,
            FitnessEquipmentData::Capabilities(CapabilitiesData {
                maximum_resistance: None,
                basic_resistance: false,
                target_power: true,
                simulation: true,
            })
        );
    }

    #[test]
    fn it_processes_page_71_after_target_power_command() {
        let payload = message::DataPayload {
            channel: 0,
            data: Some([71, 49, 1, 0, 255, 255, 200, 0]),
            channel_id: None,
            rssi: None,
            rx_timestamp: None,
        };

        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 0,
        });
        assert_eq!(fe.process_data(payload), Ok(()));
        let data = receiver.try_recv().unwrap();
        assert_eq!(
            data,
            FitnessEquipmentData::CommandStatus(CommandStatusData {
                command_id: 49,
                sequence_no: 1,
                command_status: CommandStatus::Pass,
                total_resistance: None,
                target_power: Some(200),
                wind_resistance_coefficient: None,
                wind_speed: None,
                drafting_factor: None,
                grade: None,
                rolling_resistance_coefficient: None,
            })
        );
    }

    #[test]
    fn it_processes_page_71_after_wind_resistance_command() {
        let payload = message::DataPayload {
            channel: 0,
            data: Some([71, 50, 1, 0, 255, 40, 127, 100]),
            channel_id: None,
            rssi: None,
            rx_timestamp: None,
        };

        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 0,
        });
        assert_eq!(fe.process_data(payload), Ok(()));
        let data = receiver.try_recv().unwrap();
        assert_eq!(
            data,
            FitnessEquipmentData::CommandStatus(CommandStatusData {
                command_id: 50,
                sequence_no: 1,
                command_status: CommandStatus::Pass,
                total_resistance: None,
                target_power: None,
                wind_resistance_coefficient: Some(40),
                wind_speed: Some(127),
                drafting_factor: Some(100),
                grade: None,
                rolling_resistance_coefficient: None,
            })
        );
    }

    #[test]
    fn it_processes_page_80() {
        let payload = message::DataPayload {
            channel: 0,
            data: Some([80, 255, 255, 4, 9, 0, 65, 1]),
            channel_id: None,
            rssi: None,
            rx_timestamp: None,
        };

        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 0,
        });
        assert_eq!(fe.process_data(payload), Ok(()));
        let data = receiver
            .try_recv()
            .expect("message should have been received");
        assert_eq!(
            data,
            FitnessEquipmentData::Common(message::common::DataPage::ManufacturerInformation {
                hardware_revision: 4,
                manufacturer_id: 9,
                model_number: 321,
            }),
        );
    }

    #[test]
    fn it_processes_page_81() {
        let payload = message::DataPayload {
            channel: 0,
            data: Some([81, 255, 31, 64, 1, 14, 51, 1]),
            channel_id: None,
            rssi: None,
            rx_timestamp: None,
        };

        let (mut fe, receiver) = new_paired(DevicePairing {
            device_id: 12345,
            transmission_type: 0,
        });
        assert_eq!(fe.process_data(payload), Ok(()));
        let data = receiver
            .try_recv()
            .expect("message should have been received");
        assert_eq!(
            data,
            FitnessEquipmentData::Common(message::common::DataPage::ProductInformation {
                software_revision: 6431,
                serial_number: 20123137
            }),
        );
    }
}

use std::time::Duration;

use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::device::{DataProcessor, Device, DevicePairing, Error};
use crate::{bytes, message};

#[repr(u16)]
#[derive(Clone, Copy, Debug, IntoPrimitive, TryFromPrimitive)]
pub enum HeartRateMonitorPeriod {
    Period4Hz = 8070,
    Period2Hz = 16140,
    Period1Hz = 32280,
}

#[derive(Clone, Debug)]
pub struct HeartRateMonitor {
    pairing: DevicePairing,
    period: HeartRateMonitorPeriod,

    data: HeartRateMonitorData,

    sender: crossbeam_channel::Sender<HeartRateMonitorData>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeartRateMonitorData {
    page: Option<u8>,
    page_toggle_observed: bool,

    pub computed_heart_rate: Option<u8>,
    pub heartbeat_count: Option<u8>,
    pub heartbeat_event_time: Option<u16>,

    // data page 1
    pub cumulative_operating_time: Option<Duration>,

    // data page 2
    pub manufacturer_id: Option<u8>,
    pub serial_number: Option<u16>,

    // data page 3
    pub hardware_version: Option<u8>,
    pub software_version: Option<u8>,
    pub model_number: Option<u8>,

    // data page 4
    pub previous_heartbeat_event_time: Option<u16>,
}

impl HeartRateMonitorData {
    fn new() -> HeartRateMonitorData {
        HeartRateMonitorData {
            page: None,
            page_toggle_observed: false,
            computed_heart_rate: None,
            heartbeat_count: None,
            heartbeat_event_time: None,

            // data page 1
            cumulative_operating_time: None,

            // data page 2
            manufacturer_id: None,
            serial_number: None,

            // data page 3
            hardware_version: None,
            software_version: None,
            model_number: None,

            // data page 4
            previous_heartbeat_event_time: None,
        }
    }
}

pub fn new_search() -> (
    HeartRateMonitor,
    crossbeam_channel::Receiver<HeartRateMonitorData>,
) {
    let (sender, receiver) = crossbeam_channel::unbounded();

    let hrm = HeartRateMonitor {
        pairing: DevicePairing {
            device_id: 0,
            transmission_type: 0,
        },
        period: HeartRateMonitorPeriod::Period4Hz,

        data: HeartRateMonitorData::new(),

        sender,
    };

    (hrm, receiver)
}

pub fn new_paired(
    config: DevicePairing,
) -> (
    HeartRateMonitor,
    crossbeam_channel::Receiver<HeartRateMonitorData>,
) {
    let (sender, receiver) = crossbeam_channel::unbounded();

    let hrm = HeartRateMonitor {
        pairing: config,
        period: HeartRateMonitorPeriod::Period4Hz,

        data: HeartRateMonitorData::new(),

        sender,
    };

    (hrm, receiver)
}

impl HeartRateMonitor {
    pub fn computed_heart_rate(&self) -> Option<u8> {
        self.data.computed_heart_rate
    }
}

impl Device for HeartRateMonitor {
    fn channel_type(&self) -> message::ChannelType {
        message::ChannelType::Receive
    }

    fn device_type(&self) -> u8 {
        120
    }

    fn rf_frequency(&self) -> u8 {
        57
    }

    fn channel_period(&self) -> u16 {
        self.period.into()
    }

    fn set_channel_period(&mut self, period: u16) -> Result<(), Error> {
        match period.try_into() {
            Ok(period) => {
                self.period = period;
                Ok(())
            }
            Err(_) => Err(Error::InvalidValue),
        }
    }

    fn pairing(&self) -> DevicePairing {
        self.pairing
    }

    fn as_data_processor(&self) -> Box<dyn DataProcessor + Send> {
        Box::new(self.clone())
    }
}

impl DataProcessor for HeartRateMonitor {
    fn process_data(&mut self, data: message::BroadcastDataData) -> Result<(), Error> {
        if let Some(data) = data.data {
            if !self.data.page_toggle_observed {
                if let Some(page) = self.data.page {
                    if page & 0x80 != data[0] & 0x80 {
                        self.data.page_toggle_observed = true;
                    }
                }
            }
            self.data.page = Some(data[0]);
            self.data.heartbeat_event_time = Some(bytes::u8_to_u16(data[4], data[5]));
            self.data.heartbeat_count = Some(data[6]);
            self.data.computed_heart_rate = Some(data[7]);
            if self.data.page_toggle_observed {
                let page = data[0] & 0x7f;
                match page {
                    1 => {
                        let raw = bytes::u8_to_u32(data[1], data[2], data[3], 0);
                        self.data.cumulative_operating_time =
                            Some(Duration::from_secs((raw * 2).into()));
                    }
                    2 => {
                        self.data.manufacturer_id = Some(data[1]);
                        self.data.serial_number = Some(bytes::u8_to_u16(data[2], data[3]));
                    }
                    3 => {
                        self.data.hardware_version = Some(data[1]);
                        self.data.software_version = Some(data[2]);
                        self.data.model_number = Some(data[3]);
                    }
                    4 => {
                        self.data.previous_heartbeat_event_time =
                            Some(bytes::u8_to_u16(data[2], data[3]));
                    }
                    _ => {
                        return Err(Error::InvalidValue);
                    }
                }
            }

            self.sender.try_send(self.data)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::{new_search, HeartRateMonitorData};
    use crate::{device::DataProcessor, message};
    use core::time::Duration;

    const PAGE_1_TEST: message::BroadcastDataData = message::BroadcastDataData {
        channel: 0,
        data: Some([1, 83, 153, 1, 147, 80, 31, 73]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    };
    const PAGE_2_TEST: message::BroadcastDataData = message::BroadcastDataData {
        channel: 0,
        data: Some([2, 1, 40, 0, 33, 11, 3, 71]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    };
    const PAGE_3_TEST: message::BroadcastDataData = message::BroadcastDataData {
        channel: 0,
        data: Some([3, 4, 21, 7, 247, 75, 20, 64]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    };
    const PAGE_3_TEST_TOGGLE: message::BroadcastDataData = message::BroadcastDataData {
        channel: 0,
        data: Some([131, 4, 21, 7, 247, 75, 20, 64]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    };
    const PAGE_4_TEST: message::BroadcastDataData = message::BroadcastDataData {
        channel: 0,
        data: Some([4, 27, 222, 94, 173, 98, 26, 63]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    };
    const PAGE_4_TEST_TOGGLE: message::BroadcastDataData = message::BroadcastDataData {
        channel: 0,
        data: Some([132, 27, 222, 94, 173, 98, 26, 63]),
        channel_id: None,
        rssi: None,
        rx_timestamp: None,
    };

    //const PAGE_1_TEST: [u8; 8] = [1, 83, 153, 1, 147, 80, 31, 73];
    // const PAGE_2_TEST: [u8; 8] = [2, 1, 40, 0, 33, 11, 3, 71];
    // const PAGE_3_TEST: [u8; 8] = [3, 4, 21, 7, 247, 75, 20, 64];
    // const PAGE_3_TEST_TOGGLE: [u8; 8] = [131, 4, 21, 7, 247, 75, 20, 64];
    // const PAGE_4_TEST: [u8; 8] = [4, 27, 222, 94, 173, 98, 26, 63];
    // const PAGE_4_TEST_TOGGLE: [u8; 8] = [132, 27, 222, 94, 173, 98, 26, 63];

    #[test]
    fn it_processes_page1_standard_fields() {
        let (mut hrm, _receiver) = new_search();
        assert_eq!(hrm.process_data(PAGE_1_TEST), Ok(()));
        let mut expected = HeartRateMonitorData::new();
        expected.page = Some(1);
        expected.computed_heart_rate = Some(73);
        expected.heartbeat_event_time = Some(20627);
        expected.heartbeat_count = Some(31);
        assert_eq!(hrm.data, expected)
    }

    #[test]
    fn it_processes_page1_specific_fields_after_page_change_toggle() {
        let (mut hrm, _receiver) = new_search();
        assert_eq!(hrm.process_data(PAGE_3_TEST_TOGGLE), Ok(()));
        assert_eq!(hrm.process_data(PAGE_1_TEST), Ok(()));
        let mut expected = HeartRateMonitorData::new();
        expected.page = Some(1);
        expected.page_toggle_observed = true;
        expected.computed_heart_rate = Some(73);
        expected.heartbeat_event_time = Some(20627);
        expected.heartbeat_count = Some(31);
        expected.cumulative_operating_time = Some(Duration::from_secs(104787 * 2));
        assert_eq!(hrm.data, expected);
    }

    #[test]
    fn it_processes_page2_specific_fields_after_page_change_toggle() {
        let (mut hrm, _receiver) = new_search();
        assert_eq!(hrm.process_data(PAGE_3_TEST_TOGGLE), Ok(()));
        assert_eq!(hrm.process_data(PAGE_2_TEST), Ok(()));
        let mut expected = HeartRateMonitorData::new();
        expected.page = Some(2);
        expected.page_toggle_observed = true;
        expected.computed_heart_rate = Some(71);
        expected.heartbeat_event_time = Some(2849);
        expected.heartbeat_count = Some(3);
        expected.manufacturer_id = Some(1);
        expected.serial_number = Some(40);
        assert_eq!(hrm.data, expected);
    }

    #[test]
    fn it_does_not_process_page3_specific_fields_before_page_change_toggle() {
        let (mut hrm, _receiver) = new_search();
        assert_eq!(hrm.process_data(PAGE_3_TEST), Ok(()));
        let mut expected = HeartRateMonitorData::new();
        expected.page = Some(3);
        expected.computed_heart_rate = Some(64);
        expected.heartbeat_event_time = Some(19447);
        expected.heartbeat_count = Some(20);
        assert_eq!(hrm.data, expected);
    }

    #[test]
    fn it_processes_page3_specific_fields_after_page_change_toggle() {
        let (mut hrm, _receiver) = new_search();
        assert_eq!(hrm.process_data(PAGE_3_TEST), Ok(()));
        assert_eq!(hrm.process_data(PAGE_3_TEST_TOGGLE), Ok(()));
        let mut expected = HeartRateMonitorData::new();
        expected.page = Some(131);
        expected.page_toggle_observed = true;
        expected.computed_heart_rate = Some(64);
        expected.heartbeat_event_time = Some(19447);
        expected.heartbeat_count = Some(20);
        expected.hardware_version = Some(4);
        expected.software_version = Some(21);
        expected.model_number = Some(7);
        assert_eq!(hrm.data, expected);
    }

    #[test]
    fn it_does_not_process_page4_specific_fields_before_page_change_toggle() {
        let (mut hrm, _receiver) = new_search();
        assert_eq!(hrm.process_data(PAGE_4_TEST), Ok(()));
        let mut expected = HeartRateMonitorData::new();
        expected.page = Some(4);
        expected.computed_heart_rate = Some(63);
        expected.heartbeat_event_time = Some(25261);
        expected.heartbeat_count = Some(26);
        assert_eq!(hrm.data, expected);
    }

    #[test]
    fn it_processes_page4_specific_fields_after_page_change_toggle() {
        let (mut hrm, _receiver) = new_search();
        assert_eq!(hrm.process_data(PAGE_4_TEST), Ok(()));
        assert_eq!(hrm.process_data(PAGE_4_TEST_TOGGLE), Ok(()));
        let mut expected = HeartRateMonitorData::new();
        expected.page = Some(132);
        expected.page_toggle_observed = true;
        expected.computed_heart_rate = Some(63);
        expected.heartbeat_event_time = Some(25261);
        expected.heartbeat_count = Some(26);
        expected.previous_heartbeat_event_time = Some(24286);
        assert_eq!(hrm.data, expected);
    }
}

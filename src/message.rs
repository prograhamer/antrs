pub mod reader;

use bitflags::bitflags;
use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::bytes;

pub const SYNC: u8 = 0xa4;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, IntoPrimitive, TryFromPrimitive)]
pub enum MessageID {
    // ChannelEvent is a special MessageID relating to a channel event, not channel response
    ChannelEvent = 0x01,

    ChannelResponseEvent = 0x40,
    AssignChannel = 0x42,
    SetChannelPeriod = 0x43,
    SetChannelRFFrequency = 0x45,
    SetNetworkKey = 0x46,
    ResetSystem = 0x4a,
    OpenChannel = 0x4b,
    CloseChannel = 0x4c,
    RequestMessage = 0x4d,
    BroadcastData = 0x4e,
    SetChannelID = 0x51,
    StartupMessage = 0x6f,
}

impl std::fmt::Display for MessageID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, IntoPrimitive, TryFromPrimitive)]
pub enum MessageCode {
    ResponseNoError = 0,

    EventRXSearchTimeout = 1,
    EventRXFail = 2,
    EventTX = 3,
    EventTransferRXFailed = 4,
    EventTransferTXCompleted = 5,
    EventTransferTXFailed = 6,
    EventChannelClosed = 7,
    EventRXFailGoToSearch = 8,
    EventChannelCollision = 9,
    EventTransferTXStart = 10,
    EventTransferNextDataBlock = 17,

    ChannelInWrongState = 21,
    ChannelNotOpened = 22,
    ChannelIDNotSet = 24,
    CloseAllChannels = 25,
    TransferInProgress = 31,
    TransferSequenceNumberError = 32,
    TransferInError = 33,
    MessageSizeExceedsLimit = 39,
    InvalidMessage = 40,
    InvalidNetworkNumber = 41,
    InvalidListID = 48,
    InvalidScanTXChannel = 49,
    InvalidParameterProvided = 51,
    EventSerialQueOverflow = 52,
    EventQueOverflow = 53,
    EncryptNegotiationSuccess = 56,
    EncryptNegotiationFail = 57,
    NVMFullError = 64,
    NVMWriteError = 65,
    USBStringWriteFail = 112,
    MesgSerialErrorID = 174,
}

impl std::fmt::Display for MessageCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, IntoPrimitive, TryFromPrimitive)]
pub enum ChannelType {
    Receive = 0x00,
    Transmit = 0x10,
    SharedBidirectionalReceive = 0x20,
    SharedBidirectionalTransmit = 0x30,
    ReceiveOnly = 0x40,
    TransmitOnly = 0x50,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct ChannelExtendedAssignment : u8 {
        const BACKGROUND_SCANNING = 0x01;
        const FREQUENCY_AGILITY = 0x04;
        const FAST_CHANNEL_INIT = 0x10;
        const ASYNC_TRANSMISSION = 0x20;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AssignChannelData {
    pub channel: u8,
    pub channel_type: ChannelType,
    pub network: u8,
    pub extended_assignment: ChannelExtendedAssignment,
}

impl AssignChannelData {
    fn encode(&self) -> Vec<u8> {
        vec![
            SYNC,
            4,
            MessageID::AssignChannel.into(),
            self.channel,
            self.channel_type.into(),
            self.network,
            self.extended_assignment.bits(),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BroadcastDataData {
    pub channel: u8,
    pub data: [u8; 8],
}

impl BroadcastDataData {
    fn encode(&self) -> Vec<u8> {
        vec![
            SYNC,
            9,
            MessageID::BroadcastData.into(),
            self.channel,
            self.data[0],
            self.data[1],
            self.data[2],
            self.data[3],
            self.data[4],
            self.data[5],
            self.data[6],
            self.data[7],
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelResponseEventData {
    pub channel: u8,
    pub message_id: MessageID,
    pub message_code: MessageCode,
}

impl ChannelResponseEventData {
    fn encode(&self) -> Vec<u8> {
        vec![
            SYNC,
            3,
            MessageID::ChannelResponseEvent.into(),
            self.channel,
            self.message_id.into(),
            self.message_code.into(),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloseChannelData {
    pub channel: u8,
}

impl CloseChannelData {
    fn encode(&self) -> Vec<u8> {
        vec![SYNC, 1, MessageID::CloseChannel.into(), self.channel]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpenChannelData {
    pub channel: u8,
}

impl OpenChannelData {
    fn encode(&self) -> Vec<u8> {
        vec![SYNC, 1, MessageID::OpenChannel.into(), self.channel]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RequestMessageData {
    pub channel: u8,
    pub message_id: MessageID,
}

impl RequestMessageData {
    fn encode(&self) -> Vec<u8> {
        vec![
            SYNC,
            2,
            MessageID::RequestMessage.into(),
            self.channel,
            self.message_id.into(),
        ]
    }
}

#[derive(Debug, PartialEq)]
pub struct ResetSystem;

impl ResetSystem {
    fn encode(&self) -> Vec<u8> {
        vec![SYNC, 1, MessageID::ResetSystem.into(), 0]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetChannelIDData {
    pub channel: u8,
    pub device: u16,
    pub pairing: bool,
    pub device_type: u8,
    pub transmission_type: u8,
}

impl SetChannelIDData {
    fn encode(&self) -> Vec<u8> {
        let (device_lo, device_hi) = bytes::u16_to_u8(self.device);
        let mut device_type_byte: u8 = if self.pairing { 0x80 } else { 0x00 };
        device_type_byte |= self.device_type & 0x7f;

        vec![
            SYNC,
            5,
            MessageID::SetChannelID.into(),
            self.channel,
            device_lo,
            device_hi,
            device_type_byte,
            self.transmission_type,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetChannelPeriodData {
    pub channel: u8,
    pub period: u16,
}

impl SetChannelPeriodData {
    fn encode(&self) -> Vec<u8> {
        let (period_lo, period_hi) = bytes::u16_to_u8(self.period);
        vec![
            SYNC,
            3,
            MessageID::SetChannelPeriod.into(),
            self.channel,
            period_lo,
            period_hi,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetChannelRFFrequencyData {
    pub channel: u8,
    pub frequency: u8,
}

impl SetChannelRFFrequencyData {
    fn encode(&self) -> Vec<u8> {
        vec![
            SYNC,
            2,
            MessageID::SetChannelRFFrequency.into(),
            self.channel,
            self.frequency,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetNetworkKeyData {
    pub network: u8,
    pub key: [u8; 8],
}

impl SetNetworkKeyData {
    fn encode(&self) -> Vec<u8> {
        vec![
            SYNC,
            9,
            MessageID::SetNetworkKey.into(),
            self.network,
            self.key[0],
            self.key[1],
            self.key[2],
            self.key[3],
            self.key[4],
            self.key[5],
            self.key[6],
            self.key[7],
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StartupMessageData {
    reason: u8,
}

impl StartupMessageData {
    fn encode(&self) -> Vec<u8> {
        vec![SYNC, 1, MessageID::StartupMessage.into(), self.reason]
    }
}

#[derive(Debug, PartialEq)]
pub enum Error {
    InsufficientData,
    InvalidChannelExtendedAssignment(u8),
    InvalidChannelType(u8),
    InvalidChecksum,
    InvalidMessageCode(u8),
    InvalidMessageID(u8),
    InvalidSyncByte,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Message {
    AssignChannel(AssignChannelData),
    BroadcastData(BroadcastDataData),
    ChannelResponseEvent(ChannelResponseEventData),
    CloseChannel(CloseChannelData),
    OpenChannel(OpenChannelData),
    RequestMessage(RequestMessageData),
    ResetSystem,
    SetChannelID(SetChannelIDData),
    SetChannelPeriod(SetChannelPeriodData),
    SetChannelRFFrequency(SetChannelRFFrequencyData),
    SetNetworkKey(SetNetworkKeyData),
    StartupMessage(StartupMessageData),
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Message {
    pub fn encode(&self) -> Vec<u8> {
        let mut encoded = match self {
            Message::AssignChannel(base) => base.encode(),
            Message::BroadcastData(base) => base.encode(),
            Message::ChannelResponseEvent(base) => base.encode(),
            Message::CloseChannel(base) => base.encode(),
            Message::OpenChannel(base) => base.encode(),
            Message::RequestMessage(base) => base.encode(),
            Message::ResetSystem => ResetSystem {}.encode(),
            Message::SetChannelID(base) => base.encode(),
            Message::SetChannelPeriod(base) => base.encode(),
            Message::SetChannelRFFrequency(base) => base.encode(),
            Message::SetNetworkKey(base) => base.encode(),
            Message::StartupMessage(base) => base.encode(),
        };

        let mut checksum = 0u8;
        for b in encoded.iter() {
            checksum ^= b
        }

        encoded.push(checksum);
        encoded
    }

    pub fn decode(data: &[u8]) -> Result<Message, Error> {
        if data.len() < 5 {
            return Err(Error::InsufficientData);
        }

        if data[0] != SYNC {
            return Err(Error::InvalidSyncByte);
        }

        let data_len = data[1];
        let message_len: usize = (data_len + 4).into();

        if data.len() < message_len {
            return Err(Error::InsufficientData);
        }

        let id = match MessageID::try_from(data[2]) {
            Ok(id) => id,
            Err(_) => return Err(Error::InvalidMessageID(data[2])),
        };

        let mut calculated: u8 = 0;
        for e in &data[..message_len] {
            calculated ^= *e;
        }
        if calculated != 0 {
            return Err(Error::InvalidChecksum);
        }

        match id {
            MessageID::ChannelEvent => Err(Error::InvalidMessageID(id.into())),
            MessageID::AssignChannel => {
                let channel_type: ChannelType = match data[4].try_into() {
                    Ok(ct) => ct,
                    Err(_) => return Err(Error::InvalidChannelType(data[4])),
                };
                let extended_assignment = match ChannelExtendedAssignment::from_bits(data[6]) {
                    Some(ea) => ea,
                    None => return Err(Error::InvalidChannelExtendedAssignment(data[6])),
                };
                Ok(Message::AssignChannel(AssignChannelData {
                    channel: data[3],
                    channel_type,
                    network: data[5],
                    extended_assignment,
                }))
            }
            MessageID::BroadcastData => {
                let mut broadcast_data = [0u8; 8];
                for (i, e) in broadcast_data.iter_mut().enumerate() {
                    *e = data[4 + i];
                }
                Ok(Message::BroadcastData(BroadcastDataData {
                    channel: data[3],
                    data: broadcast_data,
                }))
            }
            MessageID::ChannelResponseEvent => {
                let message_id: MessageID = match data[4].try_into() {
                    Ok(id) => id,
                    Err(_) => return Err(Error::InvalidMessageID(data[4])),
                };
                let message_code: MessageCode = match data[5].try_into() {
                    Ok(code) => code,
                    Err(_) => return Err(Error::InvalidMessageCode(data[5])),
                };
                Ok(Message::ChannelResponseEvent(ChannelResponseEventData {
                    channel: data[3],
                    message_id,
                    message_code,
                }))
            }
            MessageID::CloseChannel => {
                Ok(Message::CloseChannel(CloseChannelData { channel: data[3] }))
            }
            MessageID::OpenChannel => {
                Ok(Message::OpenChannel(OpenChannelData { channel: data[3] }))
            }
            MessageID::RequestMessage => {
                let message_id: MessageID = match data[4].try_into() {
                    Ok(id) => id,
                    Err(_) => return Err(Error::InvalidMessageID(data[4])),
                };
                Ok(Message::RequestMessage(RequestMessageData {
                    channel: data[3],
                    message_id,
                }))
            }
            MessageID::ResetSystem => Ok(Message::ResetSystem),
            MessageID::SetChannelID => {
                let device = bytes::u8_to_u16(data[4], data[5]);
                let pairing = (data[6] & 0x80) == 0x80;
                let device_type = data[6] & 0x7f;

                Ok(Message::SetChannelID(SetChannelIDData {
                    channel: data[3],
                    device,
                    pairing,
                    device_type,
                    transmission_type: data[7],
                }))
            }
            MessageID::SetChannelPeriod => {
                let period = bytes::u8_to_u16(data[4], data[5]);

                Ok(Message::SetChannelPeriod(SetChannelPeriodData {
                    channel: data[3],
                    period,
                }))
            }
            MessageID::SetChannelRFFrequency => {
                Ok(Message::SetChannelRFFrequency(SetChannelRFFrequencyData {
                    channel: data[3],
                    frequency: data[4],
                }))
            }
            MessageID::SetNetworkKey => {
                let mut key: [u8; 8] = [0; 8];
                for (i, e) in key.iter_mut().enumerate() {
                    *e = data[4 + i];
                }
                Ok(Message::SetNetworkKey(SetNetworkKeyData {
                    network: data[3],
                    key,
                }))
            }
            MessageID::StartupMessage => Ok(Message::StartupMessage(StartupMessageData {
                reason: data[3],
            })),
        }
    }

    pub fn encoded_len(&self) -> usize {
        4 + match self {
            Message::AssignChannel(_) => 4,
            Message::BroadcastData(_) => 9,
            Message::ChannelResponseEvent(_) => 3,
            Message::CloseChannel(_) => 1,
            Message::OpenChannel(_) => 1,
            Message::RequestMessage(_) => 2,
            Message::ResetSystem => 1,
            Message::SetChannelID(_) => 5,
            Message::SetChannelPeriod(_) => 3,
            Message::SetChannelRFFrequency(_) => 2,
            Message::SetNetworkKey(_) => 9,
            Message::StartupMessage(_) => 1,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn it_encodes_assign_channel() {
        let message = Message::AssignChannel(AssignChannelData {
            channel: 2,
            channel_type: ChannelType::ReceiveOnly,
            network: 0,
            extended_assignment: ChannelExtendedAssignment::BACKGROUND_SCANNING
                | ChannelExtendedAssignment::FREQUENCY_AGILITY,
        });
        assert_eq!(
            message.encode(),
            vec![SYNC, 4, 0x42, 0x02, 0x40, 0x00, 0x05, 0xa5]
        )
    }

    #[test]
    fn it_decodes_assign_channel() {
        let data = vec![SYNC, 4, 0x42, 0x02, 0x40, 0x00, 0x01, 0xa1];
        assert_eq!(
            Message::decode(&data),
            Ok(Message::AssignChannel(AssignChannelData {
                channel: 2,
                channel_type: ChannelType::ReceiveOnly,
                network: 0,
                extended_assignment: ChannelExtendedAssignment::BACKGROUND_SCANNING
            }))
        )
    }

    #[test]
    fn it_encodes_channel_response_event() {
        let message = Message::ChannelResponseEvent(ChannelResponseEventData {
            channel: 1,
            message_id: MessageID::SetNetworkKey.into(),
            message_code: MessageCode::InvalidMessage.into(),
        });
        assert_eq!(
            message.encode(),
            vec![SYNC, 3, 0x40, 0x01, 0x46, 0x28, 0x88]
        )
    }

    #[test]
    fn it_decodes_channel_response_event() {
        let buf = [
            SYNC,
            0x03,
            MessageID::ChannelResponseEvent.into(),
            0x00,
            MessageID::SetNetworkKey.into(),
            MessageCode::ResponseNoError.into(),
            0xa1,
        ];
        let decoded = Message::decode(&buf);
        assert_eq!(
            decoded,
            Ok(Message::ChannelResponseEvent(ChannelResponseEventData {
                channel: 0,
                message_id: MessageID::SetNetworkKey,
                message_code: MessageCode::ResponseNoError,
            }))
        );
    }

    #[test]
    fn it_encodes_open_channel() {
        let message = Message::OpenChannel(OpenChannelData { channel: 2 });
        assert_eq!(message.encode(), vec![SYNC, 0x01, 0x4b, 0x02, 0xec])
    }

    #[test]
    fn it_decodes_open_channel() {
        let data = [SYNC, 0x01, 0x4b, 0x02, 0xec];
        assert_eq!(
            Message::decode(&data),
            Ok(Message::OpenChannel(OpenChannelData { channel: 2 }))
        )
    }

    #[test]
    fn it_encodes_request_message() {
        let message = Message::RequestMessage(RequestMessageData {
            channel: 2,
            message_id: MessageID::SetChannelID,
        });
        assert_eq!(message.encode(), vec![SYNC, 0x02, 0x4d, 0x02, 0x51, 0xb8])
    }

    #[test]
    fn it_decodes_request_message() {
        let data = [SYNC, 0x02, 0x4d, 0x02, 0x51, 0xb8];
        assert_eq!(
            Message::decode(&data),
            Ok(Message::RequestMessage(RequestMessageData {
                channel: 2,
                message_id: MessageID::SetChannelID
            }))
        )
    }

    #[test]
    fn it_encodes_reset_system() {
        let message = Message::ResetSystem;
        assert_eq!(message.encode(), vec![SYNC, 1, 0x4a, 0, 0xef]);
    }

    #[test]
    fn it_decodes_reset_system() {
        let data = [SYNC, 0x01, 0x4a, 0, 0xef];
        assert_eq!(Message::decode(&data), Ok(Message::ResetSystem))
    }

    #[test]
    fn it_encodes_set_channel_id() {
        let message = Message::SetChannelID(SetChannelIDData {
            channel: 2,
            device: 10231,
            pairing: true,
            device_type: 120,
            transmission_type: 0,
        });
        assert_eq!(
            message.encode(),
            vec![SYNC, 0x05, 0x51, 0x02, 0xf7, 0x27, 0xf8, 0x00, 0xda]
        )
    }

    #[test]
    fn it_decodes_set_channel_id() {
        let data = [SYNC, 0x05, 0x51, 0x02, 0xf7, 0x27, 0xf8, 0x00, 0xda];
        assert_eq!(
            Message::decode(&data),
            Ok(Message::SetChannelID(SetChannelIDData {
                channel: 2,
                device: 10231,
                pairing: true,
                device_type: 120,
                transmission_type: 0,
            }))
        )
    }

    #[test]
    fn it_encodes_set_channel_period() {
        let message = Message::SetChannelPeriod(SetChannelPeriodData {
            channel: 3,
            period: 4070,
        });
        assert_eq!(
            message.encode(),
            vec![SYNC, 0x03, 0x43, 0x03, 0xe6, 0x0f, 0x0e]
        )
    }

    #[test]
    fn it_decodes_set_channel_period() {
        let data = [SYNC, 0x03, 0x43, 0x03, 0xe6, 0x0f, 0x0e];
        assert_eq!(
            Message::decode(&data),
            Ok(Message::SetChannelPeriod(SetChannelPeriodData {
                channel: 3,
                period: 4070,
            }))
        )
    }

    #[test]
    fn it_encodes_set_channel_rf_frequency() {
        let message = Message::SetChannelRFFrequency(SetChannelRFFrequencyData {
            channel: 2,
            frequency: 57,
        });
        assert_eq!(message.encode(), vec![SYNC, 0x02, 0x45, 0x02, 0x39, 0xd8])
    }

    #[test]
    fn it_decodes_set_channel_rf_frequency() {
        let data = [SYNC, 0x02, 0x45, 0x02, 0x39, 0xd8];
        assert_eq!(
            Message::decode(&data),
            Ok(Message::SetChannelRFFrequency(SetChannelRFFrequencyData {
                channel: 2,
                frequency: 57,
            }))
        );
    }

    #[test]
    fn it_encodes_set_network_key() {
        let message = Message::SetNetworkKey(SetNetworkKeyData {
            network: 0,
            key: [9, 8, 7, 6, 5, 4, 3, 2],
        });
        assert_eq!(
            message.encode(),
            vec![SYNC, 9, 0x46, 0, 9, 8, 7, 6, 5, 4, 3, 2, 235]
        )
    }

    #[test]
    fn it_decodes_set_network_key() {
        let data = [SYNC, 9, 0x46, 0, 9, 8, 7, 6, 5, 4, 3, 2, 235];
        assert_eq!(
            Message::decode(&data),
            Ok(Message::SetNetworkKey(SetNetworkKeyData {
                network: 0,
                key: [9, 8, 7, 6, 5, 4, 3, 2]
            })),
        )
    }

    #[test]
    fn it_encodes_startup_message() {
        let message = Message::StartupMessage(StartupMessageData { reason: 0x20 });
        assert_eq!(message.encode(), vec![SYNC, 1, 0x6f, 0x20, 0xea])
    }

    #[test]
    fn it_decodes_startup_message() {
        let data = [SYNC, 0x01, MessageID::StartupMessage.into(), 0x20, 0xea];
        assert_eq!(
            Message::decode(&data),
            Ok(Message::StartupMessage(StartupMessageData { reason: 0x20 }))
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DataPage {
    CommandStatus {
        command_id: u8,
        sequence_no: u8,
        command_status: super::CommandStatus,
        response_data: [u8; 4],
    },
    ManufacturerInformation {
        hardware_revision: u8,
        manufacturer_id: u16,
        model_number: u16,
    },
    ProductInformation {
        software_revision: u16,
        serial_number: u32,
    },
}

pub fn decode(data: [u8; 8]) -> Option<DataPage> {
    match data[0] {
        71 => Some(DataPage::CommandStatus {
            command_id: data[1],
            sequence_no: data[2],
            command_status: super::CommandStatus::try_from(data[3]).ok()?,
            response_data: [data[4], data[5], data[6], data[7]],
        }),
        80 => Some(DataPage::ManufacturerInformation {
            hardware_revision: data[3],
            manufacturer_id: u16::from_le_bytes([data[4], data[5]]),
            model_number: u16::from_le_bytes([data[6], data[7]]),
        }),
        81 => {
            let mut software_revision = Into::<u16>::into(data[3]) * 100;
            if data[2] != 0xff {
                software_revision += Into::<u16>::into(data[2]);
            }

            Some(DataPage::ProductInformation {
                software_revision,
                serial_number: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            })
        }
        _ => None,
    }
}

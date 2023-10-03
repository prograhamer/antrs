use crate::message::{
    CapabilitiesAdvancedOptions, CapabilitiesAdvancedOptions2, CapabilitiesAdvancedOptions3,
    CapabilitiesAdvancedOptions4, CapabilitiesData, CapabilitiesStandardOptions,
};

#[derive(Debug)]
pub struct Capabilities {
    pub max_channels: u8,
    pub max_networks: u8,
    pub max_sensrcore_channels: u8,
    pub no_receive_channels: bool,
    pub no_transmit_channels: bool,
    pub no_receive_messages: bool,
    pub no_transmit_messages: bool,
    pub no_ackd_messages: bool,
    pub no_burst_messages: bool,
    pub network_enabled: bool,
    pub serial_number_enabled: bool,
    pub per_channel_tx_power_enabled: bool,
    pub script_enabled: bool,
    pub search_list_enabled: bool,
    pub led_enabled: bool,
    pub extended_message_enabled: bool,
    pub scan_mode_enabled: bool,
    pub proximity_search_enabled: bool,
    pub extended_assignment_enabled: bool,
    pub fs_antfs_enabled: bool,
    pub fit1_enabled: bool,
    pub advanced_burst_enabled: bool,
    pub event_buffering_enabled: bool,
    pub event_filtering_enabled: bool,
    pub high_duty_search_enabled: bool,
    pub search_sharing_enabled: bool,
    pub selective_data_updates_enabled: bool,
    pub encrypted_channel_enabled: bool,
    pub rf_active_notification_enabled: bool,
}

impl From<CapabilitiesData> for Capabilities {
    fn from(value: CapabilitiesData) -> Self {
        Self {
            max_channels: value.max_channels,
            max_networks: value.max_networks,
            max_sensrcore_channels: value.max_sensrcore_channels,

            // "standard options"
            no_receive_channels: value
                .standard_options
                .contains(CapabilitiesStandardOptions::NO_RECEIVE_CHANNELS),
            no_transmit_channels: value
                .standard_options
                .contains(CapabilitiesStandardOptions::NO_TRANSMIT_CHANNELS),
            no_receive_messages: value
                .standard_options
                .contains(CapabilitiesStandardOptions::NO_RECEIVE_MESSAGES),
            no_transmit_messages: value
                .standard_options
                .contains(CapabilitiesStandardOptions::NO_TRANSMIT_MESSAGES),
            no_ackd_messages: value
                .standard_options
                .contains(CapabilitiesStandardOptions::NO_ACKD_MESSAGES),
            no_burst_messages: value
                .standard_options
                .contains(CapabilitiesStandardOptions::NO_BURST_MESSAGES),

            // "advanced options"
            network_enabled: value
                .advanced_options
                .contains(CapabilitiesAdvancedOptions::NETWORK_ENABLED),
            serial_number_enabled: value
                .advanced_options
                .contains(CapabilitiesAdvancedOptions::SERIAL_NUMBER_ENABLED),
            per_channel_tx_power_enabled: value
                .advanced_options
                .contains(CapabilitiesAdvancedOptions::PER_CHANNEL_TX_POWER_ENABLED),
            script_enabled: value
                .advanced_options
                .contains(CapabilitiesAdvancedOptions::SCRIPT_ENABLED),
            search_list_enabled: value
                .advanced_options
                .contains(CapabilitiesAdvancedOptions::SEARCH_LIST_ENABLED),

            // "advanced options 2"
            led_enabled: value
                .advanced_options_2
                .contains(CapabilitiesAdvancedOptions2::LED_ENABLED),
            extended_message_enabled: value
                .advanced_options_2
                .contains(CapabilitiesAdvancedOptions2::EXT_MESSAGE_ENABLED),
            scan_mode_enabled: value
                .advanced_options_2
                .contains(CapabilitiesAdvancedOptions2::SCAN_MODE_ENABLED),
            proximity_search_enabled: value
                .advanced_options_2
                .contains(CapabilitiesAdvancedOptions2::PROX_SEARCH_ENABLED),
            extended_assignment_enabled: value
                .advanced_options_2
                .contains(CapabilitiesAdvancedOptions2::EXT_ASSIGN_ENABLED),
            fs_antfs_enabled: value
                .advanced_options_2
                .contains(CapabilitiesAdvancedOptions2::FS_ANTFS_ENABLED),
            fit1_enabled: value
                .advanced_options_2
                .contains(CapabilitiesAdvancedOptions2::FIT1_ENABLED),

            // "advanced options 3"
            advanced_burst_enabled: value
                .advanced_options_3
                .contains(CapabilitiesAdvancedOptions3::ADVANCED_BURST_ENABLED),
            event_buffering_enabled: value
                .advanced_options_3
                .contains(CapabilitiesAdvancedOptions3::EVENT_BUFFERING_ENABLED),
            event_filtering_enabled: value
                .advanced_options_3
                .contains(CapabilitiesAdvancedOptions3::EVENT_FILTERING_ENABLED),
            high_duty_search_enabled: value
                .advanced_options_3
                .contains(CapabilitiesAdvancedOptions3::HIGH_DUTY_SEARCH_ENABLED),
            search_sharing_enabled: value
                .advanced_options_3
                .contains(CapabilitiesAdvancedOptions3::SEARCH_SHARING_ENABLED),
            selective_data_updates_enabled: value
                .advanced_options_3
                .contains(CapabilitiesAdvancedOptions3::SELECTIVE_DATA_UPDATES_ENABLED),
            encrypted_channel_enabled: value
                .advanced_options_3
                .contains(CapabilitiesAdvancedOptions3::ENCRYPTED_CHANNEL_ENABLED),

            // "advanced options 4"
            rf_active_notification_enabled: value
                .advanced_options_4
                .contains(CapabilitiesAdvancedOptions4::RFACTIVE_NOTIFICATION_ENABLED),
        }
    }
}

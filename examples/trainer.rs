use log::{error, info};
use std::thread;
use std::time::Duration;

use antrs::device::DevicePairing;
use antrs::profile::fitness_equipment;
use antrs::{message, node};

fn main() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Trace)
        .init();

    let key: [u8; 8] = match std::env::var("ANT_NETWORK_KEY") {
        Ok(key) => match hex::decode(key) {
            Ok(key) => match key.try_into() {
                Ok(key) => key,
                Err(v) => panic!("invalid value for ANT_NETWORK_KEY: {:?}", v),
            },
            Err(e) => panic!("invalid value for ANT_NETWORK_KEY: {}", e),
        },
        Err(_) => panic!("no value for ANT_NETWORK_KEY in environment"),
    };

    let nb = node::NodeBuilder::new(key);
    let mut node = nb.build();

    if let Err(e) = node.open() {
        panic!("failed to open node: {}", e);
    }

    let (trainer, receiver) = fitness_equipment::new_paired(DevicePairing {
        device_id: 48585,
        transmission_type: 5,
    });
    let channel = match node.assign_channel(Box::new(trainer)) {
        Ok(channel) => channel,
        Err(e) => panic!("failed to assign channel: {}", e),
    };
    info!("opened channel #{}", channel);

    thread::spawn(move || loop {
        for data in receiver.iter() {
            info!("received data from trainer: {:?}", data);
        }
    });

    thread::sleep(Duration::from_secs(10));

    let request_capabilites = message::request_data_page(channel, 54);
    if let Err(e) = node.write_message(request_capabilites, Duration::from_secs(1)) {
        error!("failed to write messgae: {}", e);
    }

    thread::sleep(Duration::from_secs(10));

    let erg = fitness_equipment::target_power_message(channel, 200);
    if let Err(e) = node.write_message(erg, Duration::from_secs(1)) {
        error!("failed to write message: {}", e);
    }
    thread::sleep(Duration::from_secs(1));
    let request_command_status = message::request_data_page(channel, 71);
    if let Err(e) = node.write_message(request_command_status, Duration::from_secs(1)) {
        error!("failed to write message: {}", e);
    }

    thread::sleep(Duration::from_secs(30));

    if let Err(e) = node.close() {
        panic!("failed to close node: {}", e);
    }
}

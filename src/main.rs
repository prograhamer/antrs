use crossbeam_channel::select;
use std::thread;
use std::time::Duration;

use antrs::node;
use antrs::profile::heart_rate_monitor;

fn main() -> Result<(), node::Error> {
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

    node.open()?;

    let (hrm, hrm_receiver) = heart_rate_monitor::new_paired(antrs::device::DevicePairing {
        device_id: 47330,
        transmission_type: 1,
    });
    node.assign_channel(Box::new(hrm))?;

    let (hrm, hrm_receiver2) = heart_rate_monitor::new_paired(antrs::device::DevicePairing {
        device_id: 34164,
        transmission_type: 81,
    });
    node.assign_channel(Box::new(hrm))?;

    thread::spawn(move || loop {
        select! {
            recv(hrm_receiver) -> data => {
                if let Ok(data) = data {
                    if let Some(hr) = data.computed_heart_rate {
                        println!("Received data from HRM, heart rate = {}", hr);
                    }
                }
            }
            recv(hrm_receiver2) -> data => {
                if let Ok(data) = data {
                    if let Some(hr) = data.computed_heart_rate {
                        println!("Received data from HRM #2, heart rate = {}", hr);
                    }
                }
            }
        }
    });

    thread::sleep(Duration::from_secs(10));

    node.close()?;

    Ok(())
}

use std::thread;

use antrs::node;
use log::info;

fn main() -> Result<(), node::Error> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
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

    node.open()?;

    let (channel, receiver) = node.search()?;
    info!("channel {} assigned for search", channel);

    let h = thread::spawn(move || {
        for id in receiver.iter() {
            info!("received device ID: {:?}", id);
        }
        info!("receiver disconnected");
    });

    h.join().unwrap();

    if let Some((status, events)) = node.channel_status(channel) {
        info!("channel status: {:?}, events: {:?}", status, events);
    }
    node.close()?;

    Ok(())
}

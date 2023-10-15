use std::thread;

use antrs::node;

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

    let (channel, receiver) = node.search()?;

    let h = thread::spawn(move || {
        for id in receiver.iter() {
            println!("received device ID: {:?}", id);
        }
        println!("receiver disconnected");
    });

    h.join().unwrap();

    if let Some((status, events)) = node.channel_status(channel) {
        println!("channel status: {:?}, events: {:?}", status, events);
    }
    node.close()?;

    Ok(())
}

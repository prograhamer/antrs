use crate::message::{Message, MessageCode, MessageID};

pub type Matcher<T, R> = Box<dyn Fn(T) -> R + Send>;

pub fn match_channel_response(channel: u8, message_id: MessageID) -> Matcher<Message, bool> {
    Box::new(move |message| {
        if let Message::ChannelResponseEvent(data) = message {
            data.channel == channel && data.message_id == message_id
        } else {
            false
        }
    })
}

pub fn match_channel_event(channel: u8, message_code: MessageCode) -> Matcher<Message, bool> {
    Box::new(move |message| {
        if let Message::ChannelResponseEvent(data) = message {
            data.channel == channel
                && data.message_id == MessageID::ChannelEvent
                && data.message_code == message_code
        } else {
            false
        }
    })
}

pub fn match_capabilities() -> Matcher<Message, bool> {
    Box::new(|message| matches!(message, Message::Capabilities(_)))
}

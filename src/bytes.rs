/// Take a u16 and split it into u8 values, returned as (lsb, msb)
pub fn u16_to_u8(value: u16) -> (u8, u8) {
    let msb: u8 = ((value & 0xff00) >> 8).try_into().unwrap();
    let lsb: u8 = (value & 0xff).try_into().unwrap();
    (lsb, msb)
}

/// Take LSB and MSB u8 values and construct a u16
pub fn u8_to_u16(lsb: u8, msb: u8) -> u16 {
    let lsb: u16 = lsb.into();
    let msb: u16 = msb.into();
    (msb << 8) + lsb
}

/// Take bytes in LSB -> MSB order and construct a u32
pub fn u8_to_u32(lsb: u8, byte1: u8, byte2: u8, msb: u8) -> u32 {
    let lsb: u32 = lsb.into();
    let byte1: u32 = byte1.into();
    let byte2: u32 = byte2.into();
    let msb: u32 = msb.into();
    (msb << 24) + (byte2 << 16) + (byte1 << 8) + lsb
}

#[cfg(test)]
mod test {
    use super::{u16_to_u8, u8_to_u16, u8_to_u32};

    #[test]
    fn it_constructs_deadbeef() {
        assert_eq!(0xdeadbeef, u8_to_u32(0xef, 0xbe, 0xad, 0xde));
    }

    #[test]
    fn it_constructs_dead() {
        assert_eq!(0xdead, u8_to_u16(0xad, 0xde));
    }

    #[test]
    fn it_deconstructs_dead() {
        assert_eq!((0xad, 0xde), u16_to_u8(0xdead));
    }
}

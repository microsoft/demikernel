use super::*;

#[test]
fn wikipedia_example() {
    let bytes: [u8; 20] = [
        0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0xb8,
        0x61, 0xc0, 0xa8, 0x00, 0x01, 0xc0, 0xa8, 0x00, 0xc7,
    ];
    let mut hasher = Hasher::new();
    hasher.write(&bytes[..10]);
    hasher.write(&bytes[12..]);
    let sum = hasher.finish();
    assert_eq!(sum, 0xb861);

    let mut hasher = Hasher::new();
    hasher.write(&bytes);
    let sum = hasher.finish();
    assert_eq!(sum, 0);
}

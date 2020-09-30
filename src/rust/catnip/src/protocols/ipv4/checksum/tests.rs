// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::*;

#[test]
fn wikipedia_example() {
    let bytes: [u8; 20] = [
        0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0xb8, 0x61, 0xc0, 0xa8, 0x00,
        0x01, 0xc0, 0xa8, 0x00, 0xc7,
    ];
    let mut checksum = Ipv4Checksum::new();
    checksum.write_all(&bytes[..10]).unwrap();
    checksum.write_all(&bytes[12..]).unwrap();
    let sum = checksum.finish();
    assert_eq!(sum, 0xb861);

    let mut checksum = Ipv4Checksum::new();
    checksum.write_all(&bytes).unwrap();
    let sum = checksum.finish();
    assert_eq!(sum, 0);
}

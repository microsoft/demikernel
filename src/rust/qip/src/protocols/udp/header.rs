pub const UDP_HEADER_SIZE: usize = 8;

// todo: the `bitfield` crate has yet to implement immutable access to fields. see [this github issue](https://github.com/dzamlo/rust-bitfield/issues/23) for details.

bitfield! {
    pub struct UdpHeaderMut(MSB0 [u8]);
    impl Debug;
    u16;
    pub get_src_port, set_src_port: 15, 0;
    pub get_dst_port, set_dst_port: 31, 16;
    pub get_length, set_length: 47, 32;
    // todo: we don't yet support computing UDP checksums. it's optional for IPv4 support, so it's not critical, but having it at some point is desirable.
    pub get_checksum, set_checksum: 63, 48;
}

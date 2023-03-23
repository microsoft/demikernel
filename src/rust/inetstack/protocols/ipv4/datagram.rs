// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    inetstack::protocols::ip::IpProtocol,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
};
use ::libc::{
    EBADMSG,
    ENOTSUP,
};
use ::std::{
    convert::{
        TryFrom,
        TryInto,
    },
    net::Ipv4Addr,
};

//==============================================================================
// Constants
//==============================================================================

/// Minimum size of IPv4 header (in bytes).
pub const IPV4_HEADER_MIN_SIZE: u16 = IPV4_DATAGRAM_MIN_SIZE;

/// Maximum size of IPv4 header (in bytes).
pub const IPV4_HEADER_MAX_SIZE: u16 = 60;

/// Minimum size for an IPv4 datagram (in bytes).
const IPV4_DATAGRAM_MIN_SIZE: u16 = 20;

/// IPv4 header length when no options are present (in 32-bit words).
const IPV4_IHL_NO_OPTIONS: u8 = (IPV4_HEADER_MIN_SIZE as u8) / 4;

/// Default time to live value.
const DEFAULT_IPV4_TTL: u8 = 255;

/// Version number for IPv4.
const IPV4_VERSION: u8 = 4;

/// IPv4 Control Flag: Datagram has evil intent (see RFC 3514).
const IPV4_CTRL_FLAG_EVIL: u8 = 0x4;

/// IPv4 Control Flag: Don't Fragment.
const IPV4_CTRL_FLAG_DF: u8 = 0x2;

/// IPv4 Control Flag: More Fragments.
const IPV4_CTRL_FLAG_MF: u8 = 0x1;

//==============================================================================
// Structures
//==============================================================================

/// IPv4 Datagram Header
#[derive(Debug, Copy, Clone)]
pub struct Ipv4Header {
    /// Internet header version (4 bits).
    version: u8,
    /// Internet Header Length. (4 bits).
    ihl: u8,
    /// Differentiated Services Code Point (6 bits).
    dscp: u8,
    /// Explicit Congestion Notification (2 bits).
    ecn: u8,
    /// Total length of the packet including header and data (16 bits).
    #[allow(unused)]
    total_length: u16,
    /// Used to identify the datagram to which a fragment belongs (16 bits).
    identification: u16,
    /// Control flags (3 bits).
    flags: u8,
    /// Fragment offset indicates where in the datagram this fragment belongs to (13 bits).
    fragment_offset: u16,
    /// Time to Live indicates the maximum remaining time the datagram is allowed to be in the network (8 bits).
    ttl: u8,
    /// Protocol used in the data portion of the datagram (8 bits).
    protocol: IpProtocol,
    /// Header-only checksum for error detection (16 bits).
    #[allow(unused)]
    header_checksum: u16,
    // Source IP address (32 bits).
    src_addr: Ipv4Addr,
    /// Destination IP address (32 bits).
    dst_addr: Ipv4Addr,
}

//==============================================================================
// Associated Functions
//==============================================================================

/// Associated Functions for IPv4 Headers
impl Ipv4Header {
    /// Instantiates an empty IPv4 header.
    pub fn new(src_addr: Ipv4Addr, dst_addr: Ipv4Addr, protocol: IpProtocol) -> Self {
        Self {
            version: IPV4_VERSION,
            ihl: IPV4_IHL_NO_OPTIONS,
            dscp: 0,
            ecn: 0,
            total_length: IPV4_HEADER_MIN_SIZE as u16,
            identification: 0,
            flags: IPV4_CTRL_FLAG_DF,
            fragment_offset: 0,
            ttl: DEFAULT_IPV4_TTL,
            protocol,
            header_checksum: 0,
            src_addr,
            dst_addr,
        }
    }

    /// Computes the size of the target IPv4 header.
    pub fn compute_size(&self) -> usize {
        (self.ihl as usize) << 2
    }

    /// Parses a buffer into an IPv4 header and payload.
    pub fn parse(mut buf: DemiBuffer) -> Result<(Self, DemiBuffer), Fail> {
        // The datagram should be as big as the header.
        if buf.len() < (IPV4_DATAGRAM_MIN_SIZE as usize) {
            return Err(Fail::new(EBADMSG, "ipv4 datagram too small"));
        }

        // IP version number.
        let version: u8 = buf[0] >> 4;
        if version != IPV4_VERSION {
            return Err(Fail::new(ENOTSUP, "unsupported IP version"));
        }

        // Internet header length.
        let ihl: u8 = buf[0] & 0xF;
        let hdr_size: u16 = (ihl as u16) << 2;
        if hdr_size < IPV4_HEADER_MIN_SIZE as u16 {
            return Err(Fail::new(EBADMSG, "ipv4 IHL is too small"));
        }
        if buf.len() < hdr_size as usize {
            return Err(Fail::new(EBADMSG, "ipv4 datagram too small to fit in header"));
        }
        let hdr_buf: &[u8] = &buf[..hdr_size as usize];

        // Differentiated services code point.
        let dscp: u8 = hdr_buf[1] >> 2;
        if dscp != 0 {
            warn!("ignoring dscp field (dscp={:?})", dscp);
        }

        // Explicit congestion notification.
        let ecn: u8 = hdr_buf[1] & 3;
        if ecn != 0 {
            warn!("ignoring ecn field (ecn={:?})", ecn);
        }

        // Total length.
        let total_length: u16 = u16::from_be_bytes([hdr_buf[2], hdr_buf[3]]);
        if total_length < hdr_size {
            return Err(Fail::new(EBADMSG, "ipv4 datagram smaller than header"));
        }
        // NOTE: there may be padding bytes in the buffer.
        if (total_length as usize) > buf.len() {
            return Err(Fail::new(EBADMSG, "ipv4 datagram size mismatch"));
        }

        // Identification (Id).
        //
        // Note: We had a (now removed) bug here in that we were _requiring_ all incoming datagrams to have an Id field
        // of zero.  This was horribly misguided.  With the exception of datagramss where the DF (don't fragment) flag
        // is set, all IPv4 datagrams are _required_ to have a (temporally) unique identification field for datagrams
        // with the same source, destination, and protocol.  Thus we should expect most datagrams to have a non-zero Id.
        let identification: u16 = u16::from_be_bytes([hdr_buf[4], hdr_buf[5]]);

        // Control flags.
        //
        // Note: We had a (now removed) bug here in that we were _requiring_ all incoming datagrams to have the DF
        // (don't fragment) bit set.  This appears to be because we don't support fragmentation (yet anyway).  But the
        // lack of a set DF bit doesn't make a datagram a fragment.  So we should accept datagrams regardless of the
        // setting of this bit.
        let flags: u8 = hdr_buf[6] >> 5;
        // Don't accept evil datagrams (see RFC 3514).
        if flags & IPV4_CTRL_FLAG_EVIL != 0 {
            return Err(Fail::new(EBADMSG, "ipv4 datagram is marked as evil"));
        }

        // TODO: drop this check once we support fragmentation.
        if flags & IPV4_CTRL_FLAG_MF != 0 {
            warn!("fragmentation is not supported flags={:?}", flags);
            return Err(Fail::new(ENOTSUP, "ipv4 fragmentation is not supported"));
        }

        // Fragment offset.
        let fragment_offset: u16 = u16::from_be_bytes([hdr_buf[6], hdr_buf[7]]) & 0x1fff;
        // TODO: drop this check once we support fragmentation.
        if fragment_offset != 0 {
            warn!("fragmentation is not supported offset={:?}", fragment_offset);
            return Err(Fail::new(ENOTSUP, "ipv4 fragmentation is not supported"));
        }

        // Time to live.
        let time_to_live: u8 = hdr_buf[8];
        if time_to_live == 0 {
            return Err(Fail::new(EBADMSG, "ipv4 datagram too old"));
        }

        // Protocol.
        let protocol: IpProtocol = IpProtocol::try_from(hdr_buf[9])?;

        // Header checksum.
        let header_checksum: u16 = u16::from_be_bytes([hdr_buf[10], hdr_buf[11]]);
        if header_checksum == 0xffff {
            return Err(Fail::new(EBADMSG, "ipv4 checksum invalid"));
        }
        if header_checksum != Self::compute_checksum(hdr_buf) {
            return Err(Fail::new(EBADMSG, "ipv4 checksum mismatch"));
        }

        // Source address.
        let src_addr: Ipv4Addr = Ipv4Addr::new(hdr_buf[12], hdr_buf[13], hdr_buf[14], hdr_buf[15]);

        // Destination address.
        let dst_addr: Ipv4Addr = Ipv4Addr::new(hdr_buf[16], hdr_buf[17], hdr_buf[18], hdr_buf[19]);

        // Truncate datagram.
        let padding_bytes: usize = buf.len() - (total_length as usize);
        buf.adjust(hdr_size as usize)?;
        buf.trim(padding_bytes)?;

        let header: Ipv4Header = Self {
            version,
            ihl,
            dscp,
            ecn,
            total_length,
            identification,
            flags,
            fragment_offset,
            ttl: time_to_live,
            protocol,
            header_checksum,
            src_addr,
            dst_addr,
        };

        Ok((header, buf))
    }

    /// Serializes the target IPv4 header.
    pub fn serialize(&self, buf: &mut [u8], payload_len: usize) {
        let buf: &mut [u8; IPV4_HEADER_MIN_SIZE as usize] = buf
            .try_into()
            .expect("buffer should be large enough to hold an IPv4 header");

        // Version + IHL.
        buf[0] = (self.version << 4) | self.ihl;

        // DSCP + ECN.
        buf[1] = (self.dscp << 2) | (self.ecn & 3);

        // Total Length.
        buf[2..4].copy_from_slice(&(IPV4_HEADER_MIN_SIZE + (payload_len as u16)).to_be_bytes());

        // Identification.
        buf[4..6].copy_from_slice(&self.identification.to_be_bytes());

        // Flags and Fragment Offset.
        buf[6..8].copy_from_slice(&((self.flags as u16) << 13 | self.fragment_offset & 0x1fff).to_be_bytes());

        // Time to Live.
        buf[8] = self.ttl;

        // Protocol.
        buf[9] = self.protocol as u8;

        // Skip the checksum (bytes 10..12) until we finish writing the header.

        // Source Address.
        buf[12..16].copy_from_slice(&self.src_addr.octets());

        // Destination Address.
        buf[16..20].copy_from_slice(&self.dst_addr.octets());

        // Header Checksum.
        let checksum: u16 = Self::compute_checksum(buf);
        buf[10..12].copy_from_slice(&checksum.to_be_bytes());
    }

    /// Returns the source address field stored in the target IPv4 header.
    pub fn get_src_addr(&self) -> Ipv4Addr {
        self.src_addr
    }

    /// Returns the destination address field stored in the target IPv4 header.
    pub fn get_dest_addr(&self) -> Ipv4Addr {
        self.dst_addr
    }

    /// Returns the protocol field stored in the target IPv4 header.
    pub fn get_protocol(&self) -> IpProtocol {
        self.protocol
    }

    /// Computes the checksum of the target IPv4 header.
    pub fn compute_checksum(buf: &[u8]) -> u16 {
        let mut state: u32 = 0xffff;
        for i in 0..5 {
            state += u16::from_be_bytes([buf[2 * i], buf[2 * i + 1]]) as u32;
        }
        // Skip the 5th u16 since octets 10-12 are the header checksum, whose value should be zero when
        // computing a checksum.
        for i in 6..10 {
            state += u16::from_be_bytes([buf[2 * i], buf[2 * i + 1]]) as u32;
        }
        while state > 0xffff {
            state -= 0xffff;
        }
        !state as u16
    }
}

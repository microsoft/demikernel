// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{inetstack::protocols::*, runtime::types::DEMI_SGARRAY_MAXLEN};
use ::std::time::Duration;

//======================================================================================================================
// Constants
//======================================================================================================================

/// Size of the IPv4 and TCP headers that are accounted for when deriving an MSS from an MTU.
///
/// An MTU is the largest *IP datagram* that may be sent over a link, hence the Ethernet header is deliberately *not*
/// part of this sum. We use the minimum (option-free) header sizes because that is what the inetstack actually emits
/// for data segments. Note that if the stack ever starts sending TCP options on data segments (e.g., the 12-byte
/// timestamp option from RFC 7323), then this sum must grow accordingly. See RFC 6691.
pub const MSS_HEADER_OVERHEAD: usize = layer3::ipv4::IPV4_HEADER_MIN_SIZE as usize + layer4::tcp::MIN_TCP_HEADER_SIZE;

/// Default MTU Parameter for the inetstack.
///
/// Only used when the underlying configuration does not specify an MTU.
pub const DEFAULT_MTU: usize = 1500;

/// Fallback MSS Parameter for TCP.
///
/// This is the value that a TCP implementation must assume about the other end when the peer did not advertise an MSS
/// option during the handshake. See RFC 793 and RFC 1122.
pub const FALLBACK_MSS: usize = 536;

/// Minimum MSS Parameter for TCP
pub const MIN_MSS: usize = FALLBACK_MSS;

/// Maximum MSS Parameter for TCP
pub const MAX_MSS: usize = u16::MAX as usize;

/// Maximum Segment Lifetime
/// See: https://www.rfc-editor.org/rfc/rfc793.txt
pub const MSL: Duration = Duration::from_secs(2);

/// Delay timeout for TCP ACKs.
/// See: https://www.rfc-editor.org/rfc/rfc5681#section-4.2
pub const TCP_ACK_DELAY_TIMEOUT: Duration = Duration::from_millis(500);

/// Handshake timeout for tcp.
pub const TCP_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);

/// Derives the default MSS to advertise from a given MTU, as prescribed by RFC 793 and RFC 6691.
///
/// The result is clamped to [`MIN_MSS`]..=[`MAX_MSS`], so that degenerate MTUs can neither underflow nor produce a
/// value that does not fit in the 16-bit MSS option.
pub const fn default_mss_from_mtu(mtu: usize) -> usize {
    let mss: usize = match mtu.checked_sub(MSS_HEADER_OVERHEAD) {
        Some(mss) => mss,
        None => 0,
    };

    if mss < MIN_MSS {
        MIN_MSS
    } else if mss > MAX_MSS {
        MAX_MSS
    } else {
        mss
    }
}

/// Default MSS Parameter for TCP.
///
/// This is the MSS that [`TcpConfig::default`] falls back to when there is no configuration at all to read an MTU
/// from. It is the MSS derived from [`DEFAULT_MTU`], so that both defaults agree with each other.
///
/// [`TcpConfig::default`]: crate::inetstack::config::TcpConfig::default
pub const DEFAULT_MSS: usize = default_mss_from_mtu(DEFAULT_MTU);

/// Max batch size of packets for both transmit and receive up and down the stack. This is based on the
/// DEMI_SGARRAY_MAXLEN and should always be bigger than that to receiving an entire sga worth of packets at once.
pub const MAX_BATCH_SIZE_NUM_PACKETS: usize = 20;
const _: () = debug_assert!(DEMI_SGARRAY_MAXLEN <= MAX_BATCH_SIZE_NUM_PACKETS);

/// Maximum local and remote window scaling factor.
/// See: RFC 1323, Section 2.3.
pub const MAX_WINDOW_SCALE: usize = 14;

// Maximum header size of all possible headers.
pub const MAX_HEADER_SIZE: usize =
    layer4::tcp::MAX_TCP_HEADER_SIZE + layer3::ipv4::IPV4_HEADER_MAX_SIZE as usize + layer2::ETHERNET2_HEADER_SIZE;

pub const MAX_RECV_ITERS: usize = 2;

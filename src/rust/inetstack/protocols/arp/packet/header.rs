// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    memory::DemiBuffer,
    network::types::MacAddress,
};
use ::libc::{
    EBADMSG,
    ENOTSUP,
};
use ::std::{
    convert::TryInto,
    net::Ipv4Addr,
};

//======================================================================================================================
// Constants
//======================================================================================================================

const ARP_HTYPE_ETHER2: u16 = 1;
const ARP_HLEN_ETHER2: u8 = 6;
const ARP_PTYPE_IPV4: u16 = 0x800;
const ARP_PLEN_IPV4: u8 = 4;
const ARP_MESSAGE_SIZE: usize = 28;

//======================================================================================================================
// Enumerations
//======================================================================================================================

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpOperation {
    Request = 1,
    Reply = 2,
}

//======================================================================================================================
// Structures
//======================================================================================================================

///
/// # Protocol Data Unit (PDU) for ARP
///
#[derive(Clone, Debug)]
pub struct ArpHeader {
    // We only support Ethernet/Ipv4, so omit these fields.
    // hardware_type: u16,
    // protocol_type: u16,
    // hardware_address_len: u8,
    // protocol_address_len: u8,
    operation: ArpOperation,
    sender_hardware_addr: MacAddress,
    sender_protocol_addr: Ipv4Addr,
    target_hardware_addr: MacAddress,
    target_protocol_addr: Ipv4Addr,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl ArpHeader {
    /// Creates an ARP protocol data unit.
    pub fn new(
        op: ArpOperation,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        remote_link_addr: MacAddress,
        remote_ipv4_addr: Ipv4Addr,
    ) -> Self {
        Self {
            operation: op,
            sender_hardware_addr: local_link_addr,
            sender_protocol_addr: local_ipv4_addr,
            target_hardware_addr: remote_link_addr,
            target_protocol_addr: remote_ipv4_addr,
        }
    }

    /// Computes the size of the target ARP PDU.
    pub fn compute_size(&self) -> usize {
        ARP_MESSAGE_SIZE
    }

    pub fn parse(buf: DemiBuffer) -> Result<Self, Fail> {
        if buf.len() < ARP_MESSAGE_SIZE {
            return Err(Fail::new(EBADMSG, "ARP message too short"));
        }
        let buf: &[u8; ARP_MESSAGE_SIZE] = &buf[..ARP_MESSAGE_SIZE].try_into().unwrap();
        let hardware_type: u16 = u16::from_be_bytes([buf[0], buf[1]]);
        if hardware_type != ARP_HTYPE_ETHER2 {
            return Err(Fail::new(ENOTSUP, "unsupported HTYPE"));
        }
        let protocol_type: u16 = u16::from_be_bytes([buf[2], buf[3]]);
        if protocol_type != ARP_PTYPE_IPV4 {
            return Err(Fail::new(ENOTSUP, "unsupported PTYPE"));
        }
        let hardware_address_len: u8 = buf[4];
        if hardware_address_len != ARP_HLEN_ETHER2 {
            return Err(Fail::new(ENOTSUP, "unsupported HLEN"));
        }
        let protocol_address_len: u8 = buf[5];
        if protocol_address_len != ARP_PLEN_IPV4 {
            return Err(Fail::new(ENOTSUP, "unsupported PLEN"));
        }
        let operation: ArpOperation = u16::from_be_bytes([buf[6], buf[7]]).try_into()?;
        let sender_hardware_addr: MacAddress = MacAddress::from_bytes(&buf[8..14]);
        let sender_protocol_addr: Ipv4Addr = Ipv4Addr::new(buf[14], buf[15], buf[16], buf[17]);
        let target_hardware_addr: MacAddress = MacAddress::from_bytes(&buf[18..24]);
        let target_protocol_addr: Ipv4Addr = Ipv4Addr::new(buf[24], buf[25], buf[26], buf[27]);
        let pdu: ArpHeader = Self {
            operation,
            sender_hardware_addr,
            sender_protocol_addr,
            target_hardware_addr,
            target_protocol_addr,
        };
        Ok(pdu)
    }

    /// Serializes the target ARP PDU.
    pub fn serialize(&self, buf: &mut [u8]) {
        let buf: &mut [u8; ARP_MESSAGE_SIZE] = (&mut buf[..ARP_MESSAGE_SIZE]).try_into().unwrap();
        buf[0..2].copy_from_slice(&ARP_HTYPE_ETHER2.to_be_bytes());
        buf[2..4].copy_from_slice(&ARP_PTYPE_IPV4.to_be_bytes());
        buf[4] = ARP_HLEN_ETHER2;
        buf[5] = ARP_PLEN_IPV4;
        buf[6..8].copy_from_slice(&(self.operation as u16).to_be_bytes());
        buf[8..14].copy_from_slice(&self.sender_hardware_addr.octets());
        buf[14..18].copy_from_slice(&self.sender_protocol_addr.octets());
        buf[18..24].copy_from_slice(&self.target_hardware_addr.octets());
        buf[24..28].copy_from_slice(&self.target_protocol_addr.octets());
    }

    pub fn get_operation(&self) -> ArpOperation {
        self.operation
    }

    pub fn get_sender_hardware_addr(&self) -> MacAddress {
        self.sender_hardware_addr
    }

    pub fn get_sender_protocol_addr(&self) -> Ipv4Addr {
        self.sender_protocol_addr
    }

    pub fn get_destination_protocol_addr(&self) -> Ipv4Addr {
        self.target_protocol_addr
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl TryFrom<u16> for ArpOperation {
    type Error = Fail;

    /// Attempts to convert a [u16] into a [ArpOperation].
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ArpOperation::Request),
            2 => Ok(ArpOperation::Reply),
            _ => Err(Fail::new(libc::ENOTSUP, "unsupported ARP operation")),
        }
    }
}

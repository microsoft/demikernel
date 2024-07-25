// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::memory::DemiBuffer;
use super::TcpMigHeader;

//==============================================================================
// Structures
//==============================================================================

/// Wrapper over a TCPMig packet that is ordered only w.r.t. the header's `fragment_offset` field.
#[derive(Debug)]
struct Fragment(TcpMigHeader, DemiBuffer);

pub struct TcpMigDefragmenter {
    fragments: Vec<DemiBuffer>,
    total_fragments: Option<usize>,
    current_fragments: usize,
    current_size: usize,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl TcpMigDefragmenter {
    pub fn new() -> Self {
        Self {
            fragments: Vec::new(),
            total_fragments: None,
            current_fragments: 0,
            current_size: 0,
        }
    }

    /// Returns None if all the fragments have not been received.
    pub fn defragment(&mut self, hdr: TcpMigHeader, buf: DemiBuffer) -> Option<(TcpMigHeader, DemiBuffer)> {
        let offset = hdr.fragment_offset as usize;

        // Last fragment received. Now we know the total number of fragments.
        if !hdr.flag_next_fragment {
            // Optimisation for single-fragment buffers.
            if offset == 0 {
                self.reset();
                return Some((hdr, buf));
            }

            let count = offset + 1;
            self.total_fragments = Some(count);
            self.fragments.reserve(count - self.fragments.len());
        }
        
        // Store buffer at the correct position.
        if offset >= self.fragments.len() {
            let additional = offset + 1 - self.fragments.len();
            for _ in 0..additional {
                self.fragments.push(DemiBuffer::new(0));
            }
        }
        self.current_size += buf.len();
        self.current_fragments += 1;
        self.fragments[offset] = buf;

        // Return if more fragments left.
        let count = self.total_fragments?;
        if count != self.current_fragments {
            return None
        }

        let mut hdr = hdr;
        hdr.flag_next_fragment = false;
        hdr.fragment_offset = 0;

        // Defragment.
        let mut buffer = DemiBuffer::new(self.current_size as u16);
        self.fragments.iter().fold(0, |start, f| {
            let end = start + f.len();
            buffer[start..end].copy_from_slice(&f);
            end
        });

        // Reset the defragmenter and return the defragmented buffer.
        self.reset();
        
        Some((hdr, buffer))
    }

    fn reset(&mut self) {
        self.fragments.clear();
        self.total_fragments = None;
        self.current_fragments = 0;
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl PartialEq for Fragment {
    fn eq(&self, other: &Self) -> bool {
        self.0.fragment_offset == other.0.fragment_offset
    }
}

impl Eq for Fragment {}

impl PartialOrd for Fragment {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering;
        if self.0.fragment_offset == other.0.fragment_offset {
            Some(Ordering::Equal)
        } else if self.0.fragment_offset > other.0.fragment_offset {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Less)
        }
    }
}

impl Ord for Fragment {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

/* #[cfg(test)]
mod test {
    use super::*;
    use crate::runtime::memory::DataBuffer;
    use std::{net::SocketAddrV4, str::FromStr};

    fn tcpmig_header(fragment_offset: u16, flag_next_fragment: bool) -> TcpMigHeader {
        TcpMigHeader {
            origin: SocketAddrV4::from_str("198.0.0.1:20000").unwrap(),
            remote: SocketAddrV4::from_str("18.45.32.67:19465").unwrap(),
            length: 8,
            fragment_offset,
            flag_load: false,
            flag_next_fragment,
            stage: super::super::super::MigrationStage::ConnectionState,
        }
    }

    fn buffer(buf: &[u8]) -> Buffer {
        Buffer::Heap(DataBuffer::from_slice(buf))
    }

    #[test]
    fn test_tcpmig_defragmentation() {
        let mut defragmenter = TcpMigDefragmenter::new();
        assert_eq!(
            defragmenter.defragment(tcpmig_header(1, true), buffer(&[4, 5, 6])),
            None
        );

        assert_eq!(
            defragmenter.defragment(tcpmig_header(2, false), buffer(&[7, 8])),
            None
        );

        assert_eq!(
            defragmenter.defragment(tcpmig_header(0, true), buffer(&[1, 2, 3])),
            Some((
                tcpmig_header(0, false),
                buffer(&[1, 2, 3, 4, 5, 6, 7, 8])
            ))
        );
    }
}
 */
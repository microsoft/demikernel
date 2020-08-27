// from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/tcpip-illustrated-volume/9780132808200/ch13.html):
// > if no MSS option is provided, a default value of 536 bytes is used.
pub const FALLBACK_MSS: usize = 536;

pub const MIN_MSS: usize = 536;
pub const MAX_MSS: usize = u16::max_value() as usize;

// TODO: does this need to be determined through MTU discovery?
pub const DEFAULT_MSS: usize = 1450;

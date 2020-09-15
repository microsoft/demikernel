// from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/tcpip-illustrated-volume/9780132808200/ch13.html):
// > if no MSS option is provided, a default value of 536 bytes is used.
pub const FALLBACK_MSS: usize = 536;

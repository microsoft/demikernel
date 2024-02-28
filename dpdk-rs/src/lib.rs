// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(clippy:all))]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused)]

use ::std::os::raw::{c_char, c_int, c_void};

#[link(name = "inlined")]
extern "C" {
    fn rte_pktmbuf_free_(packet: *mut rte_mbuf);
    fn rte_pktmbuf_alloc_(mp: *mut rte_mempool) -> *mut rte_mbuf;
    fn rte_eth_tx_burst_(port_id: u16, queue_id: u16, tx_pkts: *mut *mut rte_mbuf, nb_pkts: u16) -> u16;
    fn rte_eth_rx_burst_(port_id: u16, queue_id: u16, rx_pkts: *mut *mut rte_mbuf, nb_pkts: u16) -> u16;
    fn rte_mbuf_refcnt_read_(m: *const rte_mbuf) -> u16;
    fn rte_mbuf_refcnt_update_(m: *mut rte_mbuf, value: i16) -> u16;
    fn rte_pktmbuf_adj_(packet: *mut rte_mbuf, len: u16) -> *mut c_char;
    fn rte_pktmbuf_trim_(packet: *mut rte_mbuf, len: u16) -> c_int;
    fn rte_pktmbuf_headroom_(m: *const rte_mbuf) -> u16;
    fn rte_pktmbuf_tailroom_(m: *const rte_mbuf) -> u16;
    fn rte_errno_() -> c_int;
    fn rte_pktmbuf_chain_(head: *mut rte_mbuf, tail: *mut rte_mbuf) -> c_int;
    fn rte_eth_rss_ip_() -> c_int;
    fn rte_eth_tx_offload_tcp_cksum_() -> c_int;
    fn rte_eth_tx_offload_udp_cksum_() -> c_int;
    fn rte_eth_rx_offload_tcp_cksum_() -> c_int;
    fn rte_eth_rx_offload_udp_cksum_() -> c_int;
    fn rte_eth_tx_offload_multi_segs_() -> c_int;
    fn rte_get_timer_hz_() -> u64;
    fn rte_eth_rss_tcp_() -> c_int;
    fn rte_eth_rss_udp_() -> c_int;
    fn rte_eth_tx_offload_ip_cksum_() -> c_int;
    fn rte_eth_rx_offload_ip_cksum_() -> c_int;
    fn rte_pktmbuf_prepend_(m: *mut rte_mbuf, len: u16) -> *mut c_char;
    fn rte_spinlock_init_(sl: *mut rte_spinlock_t);
    fn rte_spinlock_lock_(sl: *mut rte_spinlock_t);
    fn rte_spinlock_trylock_(sl: *mut rte_spinlock_t) -> c_int;
    fn rte_spinlock_unlock_(sl: *mut rte_spinlock_t);
    fn rte_ring_enqueue_(r: *mut rte_ring, obj: *mut c_void) -> c_int;
    fn rte_ring_dequeue_(r: *mut rte_ring, obj_p: *mut *mut c_void) -> c_int;
}

#[cfg(all(feature = "mlx5", target_os = "windows"))]
extern "C" {
    fn rte_pmd_mlx5_get_dyn_flag_names();
}

#[cfg(all(feature = "mlx5", target_os = "linux"))]
#[link(name = "rte_net_mlx5")]
extern "C" {
    fn rte_pmd_mlx5_get_dyn_flag_names();
}

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[inline(never)]
pub fn load_mlx_driver() {
    cfg_if::cfg_if! {
        if #[cfg(feature = "mlx4")] {
            // Call mlx4 function.
        } else if #[cfg(feature = "mlx5")] {
            if std::env::var("DONT_SET_THIS").is_ok() {
                unsafe {
                    rte_pmd_mlx5_get_dyn_flag_names();
                }
            }
        } else {
            compile_error!("Please select a Mellanox version.")
        }
    }
}

#[inline]
pub unsafe fn rte_pktmbuf_free(packet: *mut rte_mbuf) {
    rte_pktmbuf_free_(packet)
}

#[inline]
pub unsafe fn rte_pktmbuf_alloc(mp: *mut rte_mempool) -> *mut rte_mbuf {
    rte_pktmbuf_alloc_(mp)
}

#[inline]
pub unsafe fn rte_eth_tx_burst(port_id: u16, queue_id: u16, tx_pkts: *mut *mut rte_mbuf, nb_pkts: u16) -> u16 {
    rte_eth_tx_burst_(port_id, queue_id, tx_pkts, nb_pkts)
}

#[inline]
pub unsafe fn rte_eth_rx_burst(port_id: u16, queue_id: u16, rx_pkts: *mut *mut rte_mbuf, nb_pkts: u16) -> u16 {
    rte_eth_rx_burst_(port_id, queue_id, rx_pkts, nb_pkts)
}

#[inline]
pub unsafe fn rte_mbuf_refcnt_read(m: *const rte_mbuf) -> u16 {
    rte_mbuf_refcnt_read_(m)
}

#[inline]
pub unsafe fn rte_mbuf_refcnt_update(m: *mut rte_mbuf, value: i16) -> u16 {
    rte_mbuf_refcnt_update_(m, value)
}

#[inline]
pub unsafe fn rte_pktmbuf_adj(packet: *mut rte_mbuf, len: u16) -> *mut c_char {
    rte_pktmbuf_adj_(packet, len)
}

#[inline]
pub unsafe fn rte_pktmbuf_trim(packet: *mut rte_mbuf, len: u16) -> c_int {
    rte_pktmbuf_trim_(packet, len)
}

#[inline]
pub unsafe fn rte_pktmbuf_headroom(m: *const rte_mbuf) -> u16 {
    rte_pktmbuf_headroom_(m)
}

#[inline]
pub unsafe fn rte_pktmbuf_tailroom(m: *const rte_mbuf) -> u16 {
    rte_pktmbuf_tailroom_(m)
}

#[inline]
pub unsafe fn rte_errno() -> c_int {
    rte_errno_()
}

#[inline]
pub unsafe fn rte_pktmbuf_chain(head: *mut rte_mbuf, tail: *mut rte_mbuf) -> c_int {
    rte_pktmbuf_chain_(head, tail)
}

#[inline]
pub unsafe fn rte_eth_rss_ip() -> c_int {
    rte_eth_rss_ip_()
}

#[inline]
pub unsafe fn rte_eth_tx_offload_tcp_cksum() -> c_int {
    rte_eth_tx_offload_tcp_cksum_()
}

#[inline]
pub unsafe fn rte_eth_rx_offload_tcp_cksum() -> c_int {
    rte_eth_rx_offload_tcp_cksum_()
}

#[inline]
pub unsafe fn rte_eth_tx_offload_udp_cksum() -> c_int {
    rte_eth_tx_offload_udp_cksum_()
}

#[inline]
pub unsafe fn rte_eth_rx_offload_udp_cksum() -> c_int {
    rte_eth_rx_offload_udp_cksum_()
}

#[inline]
pub unsafe fn rte_eth_tx_offload_multi_segs() -> c_int {
    rte_eth_tx_offload_multi_segs_()
}

#[inline]
pub unsafe fn rte_get_timer_hz() -> u64 {
    rte_get_timer_hz_()
}

#[inline]
pub unsafe fn rte_eth_rss_tcp() -> c_int {
    rte_eth_rss_tcp_()
}

#[inline]
pub unsafe fn rte_eth_rss_udp() -> c_int {
    rte_eth_rss_udp_()
}

#[inline]
pub unsafe fn rte_eth_tx_offload_ip_cksum() -> c_int {
    rte_eth_tx_offload_ip_cksum_()
}

#[inline]
pub unsafe fn rte_eth_rx_offload_ip_cksum() -> c_int {
    rte_eth_rx_offload_ip_cksum_()
}

#[inline]
pub unsafe fn rte_pktmbuf_prepend(m: *mut rte_mbuf, len: u16) -> *mut c_char {
    rte_pktmbuf_prepend_(m, len)
}

#[inline]
pub unsafe fn rte_spinlock_init(sl: *mut rte_spinlock_t) {
    rte_spinlock_init_(sl)
}

#[inline]
pub unsafe fn rte_spinlock_lock(sl: *mut rte_spinlock_t) {
    rte_spinlock_lock_(sl)
}

#[inline]
pub unsafe fn rte_spinlock_trylock(sl: *mut rte_spinlock_t) -> c_int {
    rte_spinlock_trylock_(sl)
}

#[inline]
pub unsafe fn rte_spinlock_unlock(sl: *mut rte_spinlock_t) {
    rte_spinlock_unlock_(sl)
}

#[inline]
pub unsafe fn rte_ring_enqueue(r: *mut rte_ring, obj: *mut c_void) -> c_int {
    rte_ring_enqueue_(r, obj)
}

#[inline]
pub unsafe fn rte_ring_dequeue(r: *mut rte_ring, obj_p: *mut *mut c_void) -> c_int {
    rte_ring_dequeue_(r, obj_p)
}

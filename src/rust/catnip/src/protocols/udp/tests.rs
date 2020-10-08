// // Copyright (c) Microsoft Corporation.
// // Licensed under the MIT license.

// use super::datagram::UdpDatagramDecoder;
// use crate::runtime::Runtime;
// use crate::{
//     protocols::{
//         icmpv4,
//         ip,
//     },
//     test_helpers,
// };
// use futures::{
//     task::{
//         noop_waker_ref,
//         Context,
//     },
//     FutureExt,
// };
// use must_let::must_let;
// use std::{
//     convert::TryFrom,
//     future::Future,
//     task::Poll,
//     time::{
//         Duration,
//         Instant,
//     },
// };

// #[test]
// #[ignore]
// fn unicast() {
//     // ensures that a UDP cast succeeds.

//     let alice_port = ip::Port::try_from(54321).unwrap();
//     let bob_port = ip::Port::try_from(12345).unwrap();

//     let now = Instant::now();
//     let text = vec![0xffu8; 10];
//     let alice = test_helpers::new_alice(now);
//     let mut bob = test_helpers::new_bob(now);
//     bob.open_udp_port(bob_port);

//     let mut ctx = Context::from_waker(noop_waker_ref());
//     let mut fut = alice
//         .udp_cast(test_helpers::BOB_IPV4, bob_port, alice_port, text.clone())
//         .boxed_local();
//     let now = now + Duration::from_micros(1);
//     must_let!(let Poll::Ready(..) = Future::poll(fut.as_mut(), &mut ctx));

//     let udp_datagram = {
//         alice.rt().advance_clock(now);
//         let bytes = alice.rt().pop_frame();
//         let _ = UdpDatagramDecoder::attach(&bytes).unwrap();
//         bytes
//     };

//     info!("passing UDP datagram to bob...");
//     bob.receive(&udp_datagram).unwrap();
//     bob.rt().advance_clock(now);

//     todo!();
//     // let datagram = bob.rt().pop_frame();
//     // assert_eq!(
//     //     datagram.src_ipv4_addr.unwrap(),
//     //     test_helpers::ALICE_IPV4
//     // );
//     // assert_eq!(datagram.src_port.unwrap(), alice_port);
//     // assert_eq!(datagram.dest_port.unwrap(), bob_port);
//     // assert_eq!(text.as_slice(), &datagram.payload[..text.len()]);
// }

// #[test]
// #[ignore]
// fn destination_port_unreachable() {
//     // ensures that a UDP cast succeeds.
//     let alice_port = ip::Port::try_from(54321).unwrap();
//     let bob_port = ip::Port::try_from(12345).unwrap();

//     let now = Instant::now();
//     let text = vec![0xffu8; 10];
//     let mut alice = test_helpers::new_alice(now);
//     let mut bob = test_helpers::new_bob(now);

//     let mut ctx = Context::from_waker(noop_waker_ref());
//     let mut fut = alice
//         .udp_cast(test_helpers::BOB_IPV4, bob_port, alice_port, text.clone())
//         .boxed_local();
//     assert!(Future::poll(fut.as_mut(), &mut ctx).is_ready());

//     let now = now + Duration::from_micros(1);
//     bob.rt().advance_clock(now);

//     let udp_datagram = {
//         alice.rt().advance_clock(now);
//         let bytes = alice.rt().pop_frame();
//         let _ = UdpDatagramDecoder::attach(&bytes).unwrap();
//         bytes
//     };

//     info!("passing UDP datagram to bob...");
//     bob.receive(&udp_datagram).unwrap();
//     bob.rt().advance_clock(now);
//     let icmpv4_datagram = {
//         let bytes = bob.rt().pop_frame();
//         let _ = icmpv4::Error::attach(&bytes).unwrap();
//         bytes
//     };

//     info!("passing ICMPv4 datagram to alice...");
//     alice.receive(&icmpv4_datagram).unwrap();
//     alice.rt().advance_clock(now);

//     todo!();
//     // must_let!(let Icmpv4Error { ref id, ref next_hop_mtu, .. } = &*event);
//     // assert_eq!(
//     //     id,
//     //     &icmpv4::ErrorId::DestinationUnreachable(
//     //         icmpv4::DestinationUnreachable::DestinationPortUnreachable
//     //     )
//     // );
//     // assert_eq!(next_hop_mtu, &0u16);
//     // todo: validate `context`
// }

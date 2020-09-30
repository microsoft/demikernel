- Correctness
  [X] Get rid of all unsafe code
  [ ] RST handling
  [ ] 2*MSL wait on active close
- Features
  [ ] TCP Fast Open
  [ ] Nagle's algorithm (optional)
  [ ] Silly window syndrome
  [ ] Fast retransmit
  [ ] Congestion control
  [ ] SACKs
  [ ] Delayed ACKs for full segments
  [ ] TCP Timestamps
- Performance
  [ ] Fast path for TCP receive
  [ ] Fast path for TCP send
  [ ] Remove Event indirection
  [ ] Use `bytes` for buffer management
  [ ] Use intrusive pairing heap for timers
- Memory usage
  [ ] Better data structure for open ports
  [ ] Slim down TCP control block
  [ ] Slim down TCP background workers
- Testing
  [ ] Determinized hashmap
  [ ] Randomized testing scenarios
    [ ] Random number of clients, connecting and disconnecting from each other randomly
    [ ] Futzing for sending data, receiving data, half-closing, full-closing
    [ ] Delay, drop, reorder, and duplicate packets randomly
    [ ] Different MSS and TCP options
    [ ] Invariant checks around resources getting freed
  [ ] Fuzzing (see goffrie@'s PR from h2)
  [ ] Resource limit scenarios

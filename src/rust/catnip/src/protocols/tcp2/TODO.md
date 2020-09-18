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
  [ ] Different scenarios
  [ ] Randomized testing
  [ ] Fuzzing (see goffrie@'s PR from h2)

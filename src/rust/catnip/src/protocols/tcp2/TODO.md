- Correctness
  [ ] Get rid of all unsafe code
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
- Testing
  [ ] Determinized hashmap
  [ ] Different scenarios
  [ ] Randomized testing
  [ ] Fuzzing (see goffrie@'s PR from h2)

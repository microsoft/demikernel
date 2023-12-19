// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

%start Script
%expect-unused Unmatched "UNMATCHED"

%%
Script -> Option<glue::Event>
      : {
            None
      }
      | Event {
            Some($1)
      }
      ;

Event -> glue::Event
      : EventTime Action {
            glue::Event {
                  time: $1,
                  action: $2,
            }
      }
      ;

EventTime -> std::time::Duration
      : Time {
            eprintln!("absolute time is not supported");
            std::time::Duration::from_secs(0)
      }
      | 'PLUS' Time { $2 }
      ;

Time -> std::time::Duration
      : 'FLOAT' {
            let v = $1.map_err(|_| ()).expect("failed to get float symbol");
            let s = $lexer.span_str(v.span());
            let f = s.parse::<f64>().expect("failed to parse float number");
            let secs = f.trunc() as u64;
            let nanos = (f.fract() * 1e9) as u32;
            std::time::Duration::new(secs, nanos)
      }
      | 'INTEGER' {
            let v = $1.map_err(|_| ()).expect("failed to get integer symbol");
            let s = $lexer.span_str(v.span());
            let i = s.parse::<u64>().expect("failed to parse integer number");
            std::time::Duration::from_secs(i)
      }
      ;

Action -> glue::Action
      : SyscallEvent {
            glue::Action::SyscallEvent($1)
      }
      | PacketEvent {
            glue::Action::PacketEvent($1)
      }
      ;

SyscallEvent -> glue::SyscallEvent
      : OptionalEndTime Syscall {
            glue::SyscallEvent{
                  end_time: $1,
                  syscall: $2
            }
      }
      ;

Syscall -> DemikernelSyscall
      : 'SOCKET' 'LPAREN' SocketArgs 'RPAREN' 'EQUALS' Expression {
            let ret = glue::parse_int(&$6).unwrap();
            DemikernelSyscall::Socket($3, ret)
      }
      | 'BIND' 'LPAREN' BindArgs 'RPAREN' 'EQUALS' Expression {
            let ret = glue::parse_int(&$6).unwrap();
            DemikernelSyscall::Bind($3, ret)
      }
      | 'LISTEN' 'LPAREN' ListenArgs 'RPAREN' 'EQUALS' Expression{
            let ret = glue::parse_int(&$6).unwrap();
            DemikernelSyscall::Listen($3, ret)
      }
      | 'ACCEPT' 'LPAREN' AcceptArgs 'RPAREN' 'EQUALS' Expression {
            let ret = glue::parse_int(&$6).unwrap();
            DemikernelSyscall::Accept($3, ret)
      }
      | 'CONNECT' 'LPAREN' ConnectArgs 'RPAREN' 'EQUALS' Expression {
            let ret = glue::parse_int(&$6).unwrap();
            DemikernelSyscall::Connect($3, ret)
      }
      | 'SEND' 'LPAREN' WriteArgs 'RPAREN' 'EQUALS' Expression {
            let ret = glue::parse_int(&$6).unwrap();
            DemikernelSyscall::Push($3, ret)
      }
      | 'RECV' 'LPAREN' SyscallArgs 'RPAREN' 'EQUALS' Expression {
            let ret = glue::parse_int(&$6).unwrap();
            DemikernelSyscall::Pop(ret)
      }
      | 'CLOSE' 'LPAREN' SyscallArgs 'RPAREN' 'EQUALS' Expression {
            DemikernelSyscall::Close
      }
      | 'WRITE' 'LPAREN' WriteArgs 'RPAREN' 'EQUALS' Expression {
            let ret = glue::parse_int(&$6).unwrap();
            DemikernelSyscall::Push($3, ret)
      }
      | 'READ' 'LPAREN' SyscallArgs 'RPAREN' 'EQUALS' Expression {
            let ret = glue::parse_int(&$6).unwrap();
            DemikernelSyscall::Pop(ret)
      }
      | 'WAIT' 'LPAREN' WaitArgs 'RPAREN' 'EQUALS' Expression {
            let ret = glue::parse_int(&$6).unwrap();
            DemikernelSyscall::Wait($3, ret)
      }
      | 'GETSOCKOPT' 'LPAREN' SyscallArgs 'RPAREN' 'EQUALS' Expression {
            DemikernelSyscall::Unsupported
      }
      | 'SETSOCKOPT' 'LPAREN' SyscallArgs 'RPAREN' 'EQUALS' Expression {
            DemikernelSyscall::Unsupported
      }
      ;

SocketArgs -> glue::SocketArgs
      : 'ELLIPSIS' 'COMMA' 'SOCK_STREAM' 'COMMA' 'IPPROTO_TCP' {
            glue::SocketArgs {
                  domain: glue::SocketDomain::AF_INET,
                  typ: glue::SocketType::SOCK_STREAM,
                  protocol: glue::SocketProtocol::IPPROTO_TCP,
            }
      }
      ;

BindArgs -> glue::BindArgs
      : 'INTEGER' 'COMMA' 'ELLIPSIS' 'COMMA' 'ELLIPSIS' {
            let qd = {
                  let v = $1.map_err(|_| ()).unwrap();
                  glue::parse_int($lexer.span_str(v.span())).unwrap()
            };
            glue::BindArgs {
                  qd: Some(qd),
                  addr: None,
            }
      }
      ;

ListenArgs -> glue::ListenArgs
      : 'INTEGER' 'COMMA' 'INTEGER' {
            let qd = {
                  let v = $1.map_err(|_| ()).unwrap();
                  glue::parse_int($lexer.span_str(v.span())).unwrap()
            };
            let backlog = {
                  let v = $3.map_err(|_| ()).unwrap();
                  glue::parse_int($lexer.span_str(v.span())).unwrap()
            } as usize;
            glue::ListenArgs {
                  qd: Some(qd),
                  backlog: Some(backlog),
            }
      }
      ;

AcceptArgs -> glue::AcceptArgs
      : 'INTEGER' 'COMMA' 'ELLIPSIS' 'COMMA' 'ELLIPSIS' {
            let qd = {
                  let v = $1.map_err(|_| ()).unwrap();
                  glue::parse_int($lexer.span_str(v.span())).unwrap()
            };
            glue::AcceptArgs {
                  qd: Some(qd),
            }
      }
      ;

ConnectArgs -> glue::ConnectArgs
      : 'INTEGER' 'COMMA' 'ELLIPSIS' 'COMMA' 'ELLIPSIS' {
            let qd = {
                  let v = $1.map_err(|_| ()).unwrap();
                  glue::parse_int($lexer.span_str(v.span())).unwrap()
            };
            glue::ConnectArgs{
                  qd: Some(qd),
                  addr: None,
            }
      }
      ;

WriteArgs -> glue::PushArgs
      : 'INTEGER' 'COMMA' 'ELLIPSIS' 'COMMA' 'INTEGER' {
            let qd = {
                  let v = $1.map_err(|_| ()).unwrap();
                  glue::parse_int($lexer.span_str(v.span())).unwrap()
            };
            let len = {
                  let v = $5.map_err(|_| ()).unwrap();
                  glue::parse_int($lexer.span_str(v.span())).unwrap()
            };
            glue::PushArgs {
                  qd: Some(qd),
                  buf: None,
                  len: Some(len),
            }
      }
      ;

WaitArgs -> glue::WaitArgs
      : 'INTEGER' 'COMMA' 'ELLIPSIS' {
            let qd = {
                  let v = $1.map_err(|_| ()).unwrap();
                  glue::parse_int($lexer.span_str(v.span())).unwrap()
            };
            glue::WaitArgs {
                  qd: Some(qd),
                  timeout: None
            }
      }
      ;

OptionalEndTime -> Option<std::time::Duration>
      : {
            None
      }
      | 'ELLIPSIS' Time {
            Some($2)
      }
      ;

SyscallArgs -> Vec<String>
      : { let v = Vec::new(); v }
      | ExpressionList { $1 }
      ;

ExpressionList -> Vec<String>
      : Expression {
            let mut v = Vec::new();
            v.push($1);
            v
      }
      | Expression 'COMMA' ExpressionList {
            let mut v = $3;
            v.push($1);
            v
      }
      ;

Expression -> String
      : 'ELLIPSIS' {
            let v = $1.map_err(|_| ()).unwrap();
            $lexer.span_str(v.span()).to_string()
      }
      | 'IDENTIFIER' {
            let v = $1.map_err(|_| ()).unwrap();
            $lexer.span_str(v.span()).to_string()
      }
      | 'INTEGER' {
            let v = $1.map_err(|_| ()).unwrap();
            $lexer.span_str(v.span()).to_string()
      }
      | Array {
            "array".to_string()
      }
      ;

Array -> ()
      : 'LBRACKET' 'RBRACKET' { }
      | 'LBRACKET' Expression 'RBRACKET' { }
      ;

PacketEvent -> glue::PacketEvent
      : Direction TcpPacket {
            glue::PacketEvent::Tcp($1, $2)
      }
      ;

Direction -> glue::PacketDirection
      : 'LT' {
            glue::PacketDirection::Incoming
      }
      | 'GT' {
            glue::PacketDirection::Outgoing
      }
      ;

TcpPacket -> glue::TcpPacket
      : TcpFlags TcpSequenceNumber OptionalAck OptionalWindow OptionalTcpOptions {
            glue::TcpPacket{
                  flags: $1,
                  seqnum: $2,
                  ack: $3,
                  win: $4,
                  options: $5,
            }
      }
      ;

TcpFlags -> glue::TcpFlags
      : {
            glue::TcpFlags {
                  syn: false,
                  ack: false,
                  fin: false,
                  rst: false,
                  psh: false,
                  urg: false,
                  ece: false,
                  cwr: false,
            }
      }
      | 'DOT' TcpFlags {
            let mut flags = $2;
            flags.ack = true;
            flags
      }
      | 'S' TcpFlags {
            let mut flags = $2;
            flags.syn = true;
            flags
      }
      | 'P' TcpFlags {
            let mut flags = $2;
            flags.psh = true;
            flags
      }
      | 'R' TcpFlags {
            let mut flags = $2;
            flags.rst = true;
            flags
      }
      ;

TcpSequenceNumber -> glue::TcpSequenceNumber
      : 'SEQ' 'INTEGER' 'LPAREN' 'INTEGER' 'RPAREN' {
            let seq = {
                  let v = $2.map_err(|_| ()).unwrap();
                  glue::parse_int($lexer.span_str(v.span())).unwrap()
            };
            let win = {
                  let v = $4.map_err(|_| ()).unwrap();
                  glue::parse_int($lexer.span_str(v.span())).unwrap()
            };
            glue::TcpSequenceNumber {
                  seq,
                  win,
            }
      }
      ;

OptionalAck -> Option<u32>
      : {
            None
      }
      | 'ACK' 'INTEGER' {
            let v = $2.map_err(|_| ()).unwrap();
            Some(glue::parse_int($lexer.span_str(v.span())).unwrap())
      }
      ;

OptionalWindow -> Option<u32>
      : { None }
      | 'WIN' 'INTEGER' {
            let v = $2.map_err(|_| ()).unwrap();
            Some(glue::parse_int($lexer.span_str(v.span())).unwrap())
      }
      ;

OptionalTcpOptions -> Vec<glue::TcpOption>
      : {
            let v = Vec::new();
            v
      }
      | 'LT' TcpOptionsList 'GT' {
            $2
      }
      ;

TcpOptionsList -> Vec<glue::TcpOption>
      : 'ELLIPSIS' {
            let v = Vec::new();
            v
      }
      | TcpOption {
            let mut v = Vec::new();
            v.insert(0, $1);
            v
      }
      | TcpOption 'COMMA' TcpOptionsList {
            let mut v = $3;
            v.insert(0, $1);
            v
      }
      ;

TcpOption -> glue::TcpOption
      : 'NOP' {
            glue::TcpOption::Noop
      }
      | 'EOL' {
            glue::TcpOption::EndOfOptions
      }
      | 'MSS' 'INTEGER' {
            let v = $2.map_err(|_| ()).unwrap();
            let mss = glue::parse_int($lexer.span_str(v.span())).unwrap();
            glue::TcpOption::Mss(mss.try_into().unwrap())
      }
      | 'WSCALE' 'INTEGER' {
            let v = $2.map_err(|_| ()).unwrap();
            let wscale = glue::parse_int($lexer.span_str(v.span())).unwrap();
            glue::TcpOption::WindowScale(wscale.try_into().unwrap())
      }
      | 'SACKOK' {
            glue::TcpOption::SackOk
      }
      | 'TIMESTAMP' 'VAL' 'INTEGER' 'ECR' 'INTEGER' {
            let v = $3.map_err(|_| ()).unwrap();
            let tsval = glue::parse_int($lexer.span_str(v.span())).unwrap();
            let v = $5.map_err(|_| ()).unwrap();
            let tsecr = glue::parse_int($lexer.span_str(v.span())).unwrap();
            glue::TcpOption::Timestamp(tsval, tsecr)
      }
      ;

Unmatched -> ()
      : "UNMATCHED" { }
      ;
%%

use crate::glue::{self, DemikernelSyscall};

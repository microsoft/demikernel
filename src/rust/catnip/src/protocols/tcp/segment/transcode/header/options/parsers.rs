use super::TcpOption;
use nom::number::complete::{be_u16, be_u8};

named!(kind<u8>, call!(be_u8));
named!(len<u8>, call!(be_u8));

named!(
    nop<(TcpOption)>,
    map!(verify!(kind, |k| k == &1u8), |_| TcpOption::Nop)
);
named!(
    mss<TcpOption>,
    map!(
        tuple!(verify!(kind, |k| k == &2u8), len, call!(be_u16)),
        |t| TcpOption::Mss(t.2)
    )
);
named!(
    other<TcpOption>,
    map!(tuple!(verify!(kind, |k| k != &0u8), len), |t| {
        TcpOption::Other {
            kind: t.0,
            len: t.1,
        }
    })
);

named!(eol<()>, map!(verify!(kind, |k| k == &0u8), |_| ()));
named!(pub start<Vec<TcpOption>>, map!(many_till!(alt!(nop | mss | other), eol), |t| t.0));

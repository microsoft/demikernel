use custom_error::custom_error;

custom_error! {#[derive(Clone, PartialEq, Eq)] pub TcpError
    ConnectionRefused{} = "connection refused",
}

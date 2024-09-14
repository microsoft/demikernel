use arrayvec::ArrayVec;
use chrono::NaiveTime;

//==============================================================================
// Constants
//==============================================================================

#[allow(unused)]
pub(crate) const CYAN: &'static str = "\x1B[36m";
#[allow(unused)]
pub(crate) const CLEAR: &'static str = "\x1B[0m";

//==============================================================================
// Data
//==============================================================================

#[allow(unused)]
static mut DATA: Option<Vec<(i64, ArrayVec<u8, 128>)>> = None;

//==============================================================================
// Macros
//==============================================================================
#[macro_export]
macro_rules! __capy_time_log {
    ($($arg:tt)*) => {
        $crate::capylog::time_log::__push_time_log(format_args!($($arg)*));
    };
}
#[macro_export]
macro_rules! __capy_time_log_dump {
    ($dump:expr) => {
        $crate::capylog::time_log::__write_time_log_data($dump).expect("capy_time_log_dump failed");
    };
}

#[allow(unused)]
pub use __capy_time_log;
#[allow(unused)]
pub use __capy_time_log_dump;

//==============================================================================
// Structures
//==============================================================================



//==============================================================================
// Standard Library Trait Implementations
//==============================================================================



//==============================================================================
// Implementations
//==============================================================================



//==============================================================================
// Functions
//==============================================================================

#[allow(unused)]
pub fn __write_time_log_data<W: std::io::Write>(w: &mut W) -> std::io::Result<()> {
    eprintln!("\n[CAPYLOG] dumping time log data");
    let data = data();
    for (time, msg) in data.iter() {
        write!(w, "{},{}\n", time, std::str::from_utf8(&msg).unwrap())?;
    }
    Ok(())
}

#[allow(unused)]
pub fn __push_time_log(args: std::fmt::Arguments) {
    let data = data();
    if data.len() == data.capacity() {
        eprintln!("[CAPYLOG-WARN] Time log allocation");
    }
    let mut buf = ArrayVec::new();
    if let Err(e) = std::io::Write::write_fmt(&mut buf, args) {
        panic!("capy_time_log failed: {}", e);
    }
    data.push((chrono::Local::now().timestamp_nanos(), buf));
}

#[allow(unused)]
fn data() -> &'static mut Vec<(i64, ArrayVec<u8, 128>)> {
    unsafe { DATA.as_mut().expect("capy-time-log not initialised") }
}

#[allow(unused)]
pub(super) fn init() {
    unsafe {
        DATA = Some(Vec::with_capacity(8192));
    }

    eprintln!("[CAPYLOG] capy_time_log is on");
}
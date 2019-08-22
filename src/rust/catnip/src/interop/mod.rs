use libc;

#[no_mangle]
pub extern "C" fn nip_double_input(input: libc::c_int) -> libc::c_int {
    input * 2
}

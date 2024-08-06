use std::{
    io,
    path::Path,
};
use windows::core::{PCWSTR, HSTRING};
use windows::Win32::{
    UI::Shell::ShellExecuteW,
    UI::WindowsAndMessaging::SHOW_WINDOW_CMD,
    Foundation::HWND,
};

pub fn edit_file(path: &Path) -> io::Result<()> {
    let path = HSTRING::from(path.as_os_str());
    let ret = unsafe {
        ShellExecuteW( 
            HWND(0),
            PCWSTR::null(),
            &path,
            PCWSTR::null(),
            PCWSTR::null(),
            SHOW_WINDOW_CMD(0)
        )
    };
    let code = ret.0 as usize;
    if code <= 32 {
        return Err(io::Error::last_os_error());
    } else {
        return Ok(());
    }
}
// A native child closes its only stdout handle but remains alive. Node's CRT
// descriptor and libuv/Win32 standard handles need not have the same lifetime.
use std::{fs::OpenOptions, io::{BufRead, Write}, time::Duration};
fn main() {
    let record = std::env::args_os().nth(1).expect("record path");
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    assert!(line.contains("initialize"));
    OpenOptions::new().create(true).append(true).open(record).unwrap()
        .write_all(line.as_bytes()).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        // Ownership is deliberately transferred and this fixture never writes again.
        drop(unsafe { std::fs::File::from_raw_handle(std::io::stdout().as_raw_handle()) });
    }
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        drop(unsafe { std::fs::File::from_raw_fd(1) });
    }
    loop { std::thread::sleep(Duration::from_secs(1)); }
}

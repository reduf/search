cfg_if::cfg_if! {
    if #[cfg(windows)] {
        mod windows;
        pub use self::windows::*;
    } else {
        mod dummy;
        pub use self::dummy::*;
    }
}

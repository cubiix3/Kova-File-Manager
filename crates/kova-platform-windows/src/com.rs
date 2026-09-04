//! One balanced COM apartment per calling thread.
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};

struct Apartment(bool);
impl Apartment {
    fn new() -> Self {
        // SAFETY: initializes only the calling thread; a successful call,
        // including S_FALSE, is balanced by this thread-local guard's Drop.
        let result =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if result.is_err() && result.0 as u32 != 0x8001_0106 {
            tracing::warn!("COM initialization failed: {result:?}");
        }
        Self(result.is_ok())
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: thread-local destruction runs on the initializing
            // thread, after its stack-owned COM interface values are dropped.
            unsafe {
                CoUninitialize();
            }
        }
    }
}
thread_local! { static APARTMENT: Apartment = Apartment::new(); }
pub(crate) fn ensure_sta() {
    APARTMENT.with(|_| {});
}

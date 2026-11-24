//! Native COM fixtures for the WebView2 object-reading boundary.
//! These do not launch WebView2 or reproduce an Explorer drag.
use super::{reply, webview2::dropped_paths, REQUEST_KEY, UNAVAILABLE};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2File, ICoreWebView2File_Impl, ICoreWebView2ObjectCollectionView,
    ICoreWebView2ObjectCollectionView_Impl, ICoreWebView2WebMessageReceivedEventArgs,
    ICoreWebView2WebMessageReceivedEventArgs_Impl, ICoreWebView2WebMessageReceivedEventArgs2,
    ICoreWebView2WebMessageReceivedEventArgs2_Impl,
};
use windows_core::{implement, Interface, IUnknown, PWSTR};

fn failure() -> windows_core::Error {
    windows_core::Error::from_hresult(windows_core::HRESULT(windows_sys::Win32::Foundation::E_FAIL))
}

#[implement(ICoreWebView2File)]
struct File { path: String, fail: bool }
impl ICoreWebView2File_Impl for File_Impl {
    fn Path(&self, value: *mut PWSTR) -> windows_core::Result<()> {
        if self.fail || value.is_null() { return Err(failure()); }
        // SAFETY: the native reader supplies a live PWSTR output slot. The
        // COM allocator transfers its string to take_pwstr in production.
        unsafe { *value = webview2_com::CoTaskMemPWSTR::from(self.path.as_str()).take(); }
        Ok(())
    }
}
fn file(path: &str, fail: bool) -> IUnknown {
    let file: ICoreWebView2File = File { path: path.into(), fail }.into();
    file.cast().unwrap()
}

#[implement(ICoreWebView2ObjectCollectionView)]
struct Objects(Vec<IUnknown>);
impl ICoreWebView2ObjectCollectionView_Impl for Objects_Impl {
    fn Count(&self, value: *mut u32) -> windows_core::Result<()> {
        if value.is_null() { return Err(failure()); }
        // SAFETY: the native reader supplies a live count output slot.
        unsafe { *value = self.0.len().try_into().unwrap(); }
        Ok(())
    }
    fn GetValueAtIndex(&self, index: u32) -> windows_core::Result<IUnknown> {
        self.0.get(index as usize).cloned().ok_or_else(failure)
    }
}

#[implement(ICoreWebView2WebMessageReceivedEventArgs2)]
struct Args(ICoreWebView2ObjectCollectionView);
impl ICoreWebView2WebMessageReceivedEventArgs_Impl for Args_Impl {
    fn Source(&self, _: *mut PWSTR) -> windows_core::Result<()> { Err(failure()) }
    fn WebMessageAsJson(&self, _: *mut PWSTR) -> windows_core::Result<()> { Err(failure()) }
    fn TryGetWebMessageAsString(&self, _: *mut PWSTR) -> windows_core::Result<()> { Err(failure()) }
}
impl ICoreWebView2WebMessageReceivedEventArgs2_Impl for Args_Impl {
    fn AdditionalObjects(&self) -> windows_core::Result<ICoreWebView2ObjectCollectionView> { Ok(self.0.clone()) }
}
fn read(objects: Vec<IUnknown>) -> Result<Vec<String>, String> {
    let collection: ICoreWebView2ObjectCollectionView = Objects(objects).into();
    let args: ICoreWebView2WebMessageReceivedEventArgs2 = Args(collection).into();
    let base: ICoreWebView2WebMessageReceivedEventArgs = args.cast().unwrap();
    // SAFETY: all fixture COM objects are live on this thread for the call.
    unsafe { dropped_paths(&base) }
}

#[test]
fn native_com_files_preserve_unicode_long_paths_and_posted_order() {
    let paths = vec![r"C:\fixture\nested\é #.mp3".to_owned(), format!(r"C:\fixture\{}\b.mp3", "long".repeat(80))];
    assert_eq!(read(paths.iter().map(|path| file(path, false)).collect()).unwrap(), paths);
}

#[test]
fn native_com_read_and_type_failures_refuse_the_entire_drop() {
    let unsupported: ICoreWebView2ObjectCollectionView = Objects(Vec::new()).into();
    for bad in [file(r"C:\fixture\unreadable.mp3", true), unsupported.cast().unwrap()] {
        assert_eq!(read(vec![file(r"C:\fixture\first.mp3", false), bad]), Err(UNAVAILABLE.into()));
    }
}

#[test]
fn native_empty_object_and_empty_path_results_keep_the_error_reply() {
    for objects in [Vec::new(), vec![file("", false)]] {
        let paths = read(objects);
        let answer: serde_json::Value = serde_json::from_str(&reply("fixture-id", paths)).unwrap();
        assert_eq!(answer, serde_json::json!({ REQUEST_KEY: "fixture-id", "error": UNAVAILABLE }));
    }
}

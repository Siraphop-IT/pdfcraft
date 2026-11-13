/// Files delivered asynchronously: (name, bytes).
pub type Inbox = std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

/// Files that failed to arrive asynchronously (a browser `?file=` URL that couldn't be fetched):
/// `(name, error)`, reported to the user on the next frame (#173).
pub type FailedInbox = std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>;

/// A request the operating system sends the running app, outside its window: on macOS, Finder
/// double-clicks, Open With and drops on the Dock icon arrive as Apple events, not arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsEvent {
    /// Open these files.
    Open(Vec<String>),
    /// Add these files to the Combine files list, in this order, and show it: the file manager's
    /// "Combine files" entry (#585). Nothing is combined until Combine is pressed.
    Combine(Vec<String>),
    /// Quit (the Dock's Quit, logging out), asking about unsaved changes first.
    Quit,
}

/// Returns the [`OsEvent`]s that arrived since it was last called (set by the desktop app).
pub type OsEventsFn = Box<dyn FnMut() -> Vec<OsEvent>>;

/// Finds the app's other window at a point on the screen, in physical pixels.
pub type WindowAtFn = Box<dyn Fn(i32, i32) -> Option<u32>>;
/// Opens a saved file (an absolute path) as a tab in another of the app's windows.
pub type SendTabFn = Box<dyn Fn(u32, &str) -> Result<(), String>>;

/// The app's other windows, which a tab dragged out of this one can be dropped on (set by the
/// desktop app on Windows, where each window is a process of its own).
pub struct OtherWindows {
    /// The other window at a point on the screen, in physical pixels: an id for [`Self::send`].
    pub at: WindowAtFn,
    /// Open the saved file at this absolute path as a tab in that window.
    pub send: SendTabFn,
}

/// Hands a saved PDF to the page embedding PdfCraft (web): `(name, bytes, save_as)`. `save_as`
/// is `true` for Save As. Set by the web app when a host page (such as a Nextcloud app) asked
/// for saves; without it the browser downloads the file.
pub type HostSaveFn = Box<dyn Fn(&str, &[u8], bool) -> Result<(), String>>;

/// Told every frame whether any tab has unsaved work, so a host page can warn before leaving.
pub type HostDirtyFn = Box<dyn FnMut(bool)>;

pub mod backend;
pub mod batch;
pub mod compositor;
pub mod shaders;

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        WM_STATE,
        WM_TAKE_FOCUS,
        WM_TRANSIENT_FOR,

        _NET_ACTIVE_WINDOW,
        _NET_SUPPORTED,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_STATE,
        _NET_SUPPORTING_WM_CHECK,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_WINDOW_TYPE_SPLASH,
        _NET_WM_WINDOW_TYPE_UTILITY,
        _NET_WM_WINDOW_TYPE_MENU,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_WINDOW_TYPE_TOOLBAR,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_CLIENT_LIST,
        _NET_CLIENT_LIST_STACKING,
        _NET_CLIENT_INFO,
        _NET_CURRENT_DESKTOP,
        _NET_NUMBER_OF_DESKTOPS,
        _NET_DESKTOP_NAMES,
        _NET_DESKTOP_VIEWPORT,
        _NET_WM_MOVERESIZE,
        _NET_WM_STRUT,
        _NET_WM_STRUT_PARTIAL,
        _NET_WM_WINDOW_TYPE_POPUP_MENU,
        _NET_WM_WINDOW_TYPE_DROPDOWN_MENU,
        _NET_WM_WINDOW_TYPE_TOOLTIP,
        _NET_WM_WINDOW_TYPE_COMBO,
        _NET_WM_WINDOW_TYPE_NOTIFICATION,

        UTF8_STRING,
        COMPOUND_TEXT,
    }
}

// src/xcb_conn.rs
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
        _NET_WM_STATE,
        _NET_SUPPORTING_WM_CHECK,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_CLIENT_LIST,
        _NET_CLIENT_INFO,

        UTF8_STRING,
    }
}

use log::info;
use std::ffi::{c_char, CStr};
use x11::xlib::{
    Atom, Success, Window, XGetTextProperty, XTextProperty, XmbTextPropertyToTextList, XA_STRING,
    XA_WM_NAME,
};

use crate::dwm::Dwm;

impl Dwm {
    pub fn gettextprop(&mut self, w: Window, atom: Atom, text: &mut String) -> bool {
        // info!("[gettextprop]");
        unsafe {
            let mut name: XTextProperty = std::mem::zeroed();
            if XGetTextProperty(self.dpy, w, &mut name, atom) <= 0 || name.nitems <= 0 {
                return false;
            }
            *text = "".to_string();
            let mut list: *mut *mut c_char = std::ptr::null_mut();
            let mut n: i32 = 0;
            if name.encoding == XA_STRING {
                let c_str = CStr::from_ptr(name.value as *const _);
                match c_str.to_str() {
                    Ok(val) => {
                        let mut tmp = val.to_string();
                        while tmp.as_bytes().len() > self.stext_max_len {
                            tmp.pop();
                        }
                        *text = tmp;
                        // info!(
                        //     "[gettextprop]text from string, len: {}, text: {:?}",
                        //     text.len(),
                        //     *text
                        // );
                    }
                    Err(val) => {
                        info!("[gettextprop]text from string error: {:?}", val);
                        println!("[gettextprop]text from string error: {:?}", val);
                        return false;
                    }
                }
            } else if XmbTextPropertyToTextList(self.dpy, &mut name, &mut list, &mut n)
                >= Success as i32
                && n > 0
                && !list.is_null()
            {
                let c_str = CStr::from_ptr(*list);
                match c_str.to_str() {
                    Ok(val) => {
                        let mut tmp = val.to_string();
                        while tmp.as_bytes().len() > self.stext_max_len {
                            tmp.pop();
                        }
                        *text = tmp;
                        // info!(
                        //     "[gettextprop]text from string list, len: {},  text: {:?}",
                        //     text.len(),
                        //     *text
                        // );
                    }
                    Err(val) => {
                        info!("[gettextprop]text from string list error: {:?}", val);
                        println!("[gettextprop]text from string list error: {:?}", val);
                        return false;
                    }
                }
            }
        }
        true
    }

    #[allow(dead_code)]
    pub fn updatestatus(&mut self) {
        // info!("[updatestatus]");
        let mut stext = self.stext.clone();
        if !self.gettextprop(self.root, XA_WM_NAME, &mut stext) {
            self.stext = "jwm-1.0".to_string();
        } else {
            self.stext = stext;
        }
        self.drawbar(self.selmon.clone());
    }
}

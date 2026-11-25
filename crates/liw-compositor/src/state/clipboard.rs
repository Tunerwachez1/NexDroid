//! Text selection between Waydroid's composer and the host window.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use smithay::delegate_data_device;
use smithay::input::Seat;
use smithay::reexports::rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
    clear_data_device_selection, current_data_device_selection_userdata,
    request_data_device_client_selection, set_data_device_selection,
};
use smithay::wayland::selection::{SelectionHandler, SelectionSource, SelectionTarget};

use super::Compositor;

pub const MAX_CLIPBOARD_BYTES: usize = 256 * 1024;
const TIMEOUT: Duration = Duration::from_secs(2);
const PASTE_WAIT: Duration = Duration::from_millis(500);
const TEXT: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];

#[derive(Default)]
pub(super) struct Clipboard {
    pub enabled: bool,
    source: Option<String>,
    reading: Option<(UnixStream, Vec<u8>, Instant)>,
    writing: Option<(File, Arc<str>, usize, Instant)>,
    paste_deadline: Option<Instant>,
    deferred_keys: VecDeque<(u16, bool, u32)>,
}

impl Compositor {
    pub(crate) fn clipboard_enabled(&mut self, enabled: bool) {
        self.clipboard.enabled = enabled;
        if !enabled {
            self.finish_paste();
            self.clipboard = Clipboard::default();
            let own = current_data_device_selection_userdata(&self.seat).is_some();
            if own {
                clear_data_device_selection(&self.dh, &self.seat);
            }
        }
    }

    fn offer_clipboard(&mut self, text: Option<String>) -> bool {
        if !self.clipboard.enabled {
            return false;
        }
        let Some(text) =
            text.filter(|text| text.len() <= MAX_CLIPBOARD_BYTES && !text.contains('\0'))
        else {
            let own = current_data_device_selection_userdata(&self.seat).is_some();
            if own {
                clear_data_device_selection(&self.dh, &self.seat);
            }
            return false;
        };
        let unchanged = current_data_device_selection_userdata(&self.seat)
            .is_some_and(|current| **current == *text);
        if !unchanged {
            set_data_device_selection(
                &self.dh,
                &self.seat,
                TEXT.map(str::to_owned).to_vec(),
                text.into(),
            );
        }
        !unchanged
    }

    pub(crate) fn paste_clipboard(&mut self, text: Option<String>, time: u32) {
        if self.offer_clipboard(text) {
            self.clipboard.paste_deadline = Some(Instant::now() + PASTE_WAIT);
        }
        self.clipboard_key(47, true, time);
    }

    pub(crate) fn clipboard_key(&mut self, code: u16, pressed: bool, time: u32) {
        if self.clipboard.paste_deadline.is_some() && self.clipboard.deferred_keys.len() < 64 {
            self.clipboard
                .deferred_keys
                .push_back((code, pressed, time));
        } else {
            self.finish_paste();
            self.key(code, pressed, time);
        }
    }

    fn finish_paste(&mut self) {
        self.clipboard.paste_deadline = None;
        while let Some((code, pressed, time)) = self.clipboard.deferred_keys.pop_front() {
            self.key(code, pressed, time);
        }
    }

    pub(crate) fn poll_clipboard(&mut self) -> Option<String> {
        let now = Instant::now();
        if let Some((mut file, text, mut offset, deadline)) = self.clipboard.writing.take() {
            if now < deadline {
                match file.write(&text.as_bytes()[offset..]) {
                    Ok(n) => offset += n,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(_) => offset = text.len(),
                }
                if offset < text.len() {
                    self.clipboard.writing = Some((file, text, offset, deadline));
                } else {
                    drop(file);
                    self.finish_paste();
                }
            }
        }

        if self
            .clipboard
            .paste_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.finish_paste();
        }

        if let Some(mime) = self.clipboard.source.take() {
            self.clipboard.reading = None;
            let (reader, writer) = UnixStream::pair().ok()?;
            reader.set_nonblocking(true).ok()?;
            request_data_device_client_selection(&self.seat, mime, writer.into()).ok()?;
            self.clipboard.reading = Some((reader, Vec::new(), now + TIMEOUT));
        }
        let (mut reader, mut bytes, deadline) = self.clipboard.reading.take()?;
        if now >= deadline {
            return None;
        }
        match Read::by_ref(&mut reader)
            .take((MAX_CLIPBOARD_BYTES + 1 - bytes.len()) as u64)
            .read_to_end(&mut bytes)
        {
            Ok(_) if bytes.len() <= MAX_CLIPBOARD_BYTES && !bytes.contains(&0) => {
                String::from_utf8(bytes).ok()
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.clipboard.reading = Some((reader, bytes, deadline));
                None
            }
            _ => None,
        }
    }
}

impl SelectionHandler for Compositor {
    type SelectionUserData = Arc<str>;

    fn new_selection(
        &mut self,
        target: SelectionTarget,
        source: Option<SelectionSource>,
        _: Seat<Self>,
    ) {
        if target != SelectionTarget::Clipboard || !self.clipboard.enabled {
            return;
        }
        self.clipboard.reading = None;
        self.clipboard.source = source.and_then(|source| {
            let offered = source.mime_types();
            // Password managers can mark an offer as unsuitable for forwarding.
            if offered
                .iter()
                .any(|mime| mime == "x-kde-passwordManagerHint")
            {
                return None;
            }
            let mime = TEXT
                .into_iter()
                .find(|mime| offered.iter().any(|item| item == mime))?;
            Some(mime.to_owned())
        });
    }

    fn send_selection(
        &mut self,
        target: SelectionTarget,
        mime: String,
        fd: OwnedFd,
        _: Seat<Self>,
        text: &Arc<str>,
    ) {
        if self.clipboard.enabled
            && target == SelectionTarget::Clipboard
            && TEXT.contains(&mime.as_str())
        {
            if let Ok(flags) = fcntl_getfl(&fd)
                && fcntl_setfl(&fd, flags | OFlags::NONBLOCK).is_ok()
            {
                self.clipboard.writing =
                    Some((fd.into(), text.clone(), 0, Instant::now() + TIMEOUT));
            }
        }
    }
}

impl DataDeviceHandler for Compositor {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device
    }
}
impl ClientDndGrabHandler for Compositor {}
impl ServerDndGrabHandler for Compositor {}
delegate_data_device!(Compositor);

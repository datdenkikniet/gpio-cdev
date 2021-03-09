// Copyright (c) 2018 The rust-gpio-cdev Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Wrapper for asynchronous programming using Tokio.

use futures::stream::Stream;
use futures::task::{Context, Poll};
use futures::{pin_mut, ready};
use tokio::{
    fs::File,
    io::{AsyncRead, ReadBuf},
};

use std::pin::Pin;
use std::{mem, slice};

use crate::ffi;

use super::event_err;
use super::{LineEvent, LineEventHandle, Result};

/// Wrapper around a `LineEventHandle` which implements a `futures::stream::Stream` for interrupts.
///
/// # Example
///
/// The following example waits for state changes on an input line.
///
/// ```no_run
/// use futures::stream::StreamExt;
/// use gpio_cdev::{AsyncLineEventHandle, Chip, EventRequestFlags, LineRequestFlags};
///
/// async fn print_events(line: u32) -> Result<(), gpio_cdev::Error> {
///     let mut chip = Chip::new("/dev/gpiochip0")?;
///     let line = chip.get_line(line)?;
///     let mut events = AsyncLineEventHandle::new(line.events(
///         LineRequestFlags::INPUT,
///         EventRequestFlags::BOTH_EDGES,
///         "gpioevents",
///     )?)?;
///
///     loop {
///         match events.next().await {
///             Some(event) => println!("{:?}", event?),
///             None => break,
///         };
///     }
///
///     Ok(())
/// }
///
/// # #[tokio::main]
/// # async fn main() {
/// #     print_events(42).await.unwrap();
/// # }
/// ```
pub struct AsyncLineEventHandle {
    file: File,
}

impl AsyncLineEventHandle {
    /// Wraps the specified `LineEventHandle`.
    ///
    /// # Arguments
    ///
    /// * `handle` - handle to be wrapped.
    pub fn new(handle: LineEventHandle) -> Result<AsyncLineEventHandle> {
        Ok(AsyncLineEventHandle {
            file: File::from_std(handle.file),
        })
    }

    pub(crate) fn read_event(
        bytes_read: usize,
        buffer: &mut [u8],
    ) -> std::result::Result<Option<LineEvent>, nix::Error> {
        let mut data: ffi::gpioevent_data = unsafe { mem::zeroed() };
        let data_as_buf = unsafe {
            slice::from_raw_parts_mut(
                &mut data as *mut ffi::gpioevent_data as *mut u8,
                mem::size_of::<ffi::gpioevent_data>(),
            )
        };

        data_as_buf.copy_from_slice(&buffer[..bytes_read]);

        if bytes_read != mem::size_of::<ffi::gpioevent_data>() {
            Ok(None)
        } else {
            Ok(Some(LineEvent(data)))
        }
    }
}

impl Stream for AsyncLineEventHandle {
    type Item = Result<LineEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let handle = Pin::into_inner(self);
        let file = &mut handle.file;

        pin_mut!(file);

        let mut raw_buffer = [0u8; mem::size_of::<ffi::gpioevent_data>()];
        let mut buffer = ReadBuf::new(&mut raw_buffer);
        if let Err(e) = ready!(file.poll_read(cx, &mut buffer)) {
            return Poll::Ready(Some(Err(e.into())));
        }
        match AsyncLineEventHandle::read_event(buffer.filled().len(), &mut raw_buffer) {
            Ok(Some(event)) => Poll::Ready(Some(Ok(event))),
            Ok(None) => Poll::Ready(Some(Err(event_err(nix::Error::Sys(
                nix::errno::Errno::EIO,
            ))))),
            Err(nix::Error::Sys(nix::errno::Errno::EAGAIN)) => Poll::Pending,
            Err(e) => Poll::Ready(Some(Err(event_err(e)))),
        }
    }
}

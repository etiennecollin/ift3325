//! A sliding window for the HDLC protocol.

use crate::{frame::Frame, io::create_frame_timer};
use log::debug;
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
};
use tokio::{sync::mpsc::Sender, task::JoinHandle};

/// Type alias for a safe window that can be shared and mutated between threads.
pub type SafeWindow = Arc<Mutex<Window>>;
/// Type alias for a condition variable that can be safely shared between threads.
pub type SafeCond = Arc<Condvar>;
/// Type alias for a window element that contains a frame and the handle to a timer task.
pub type WindowElement = (Frame, JoinHandle<()>);

/// Defines the error type for the window
#[derive(Debug)]
pub enum WindowError {
    /// The window is full and cannot accept any more frames
    Full,
}

/// A sliding window for a Go-Back-N protocol.
/// It implements a deque with a maximum size of `WINDOW_SIZE`.
pub struct Window {
    /// The frames in the window
    pub frames: VecDeque<WindowElement>,
    /// Flag to resend all frames in the window
    pub resend_all: bool,
    /// Flag to indicate if the connection is established
    pub is_connected: bool,
    /// Flag to indicate if the window is using the selective reject protocol
    pub srej: bool,
    /// Flag to indicate if a disconnect request was sent
    pub sent_disconnect_request: bool,
}

impl Window {
    /// Number of bits used for numbering frames
    pub const NUMBERING_BITS: usize = 3;
    /// The maximum number a frame can take
    pub const MAX_FRAME_NUM: u8 = 1 << Self::NUMBERING_BITS;
    /// The maximum time in seconds to wait before a fame is considered lost
    pub const FRAME_TIMEOUT: u64 = 3;
    /// The maximum number of frames that can be in the window for the go-back-n protocol
    const SIZE_GO_BACK_N: usize = (1 << Self::NUMBERING_BITS) - 1;
    /// The maximum number of frames that can be in the window for the selective reject protocol
    const SIZE_SREJ: usize = 1 << (Self::NUMBERING_BITS - 1);

    /// Create a new window
    /// The window is initially empty and has a capacity of `WINDOW_SIZE`
    pub fn new() -> Self {
        Self {
            frames: VecDeque::with_capacity(Self::SIZE_GO_BACK_N),
            resend_all: false,
            is_connected: false,
            srej: false,
            sent_disconnect_request: false,
        }
    }

    /// Get the maximum number of frames that can be in the window
    pub fn get_max_size(&self) -> usize {
        if self.srej {
            Self::SIZE_SREJ
        } else {
            Self::SIZE_GO_BACK_N
        }
    }

    /// Push a frame to the back of the window and start a timer to resend it
    /// if needed.
    ///
    /// # Arguments
    /// - `frame`: The frame to push to the window
    /// - `writer_tx`: The channel the timer uses to send the frame to the writer task
    ///
    /// # Errors
    /// If the window is full, an error is returned
    pub fn push(&mut self, frame: Frame, writer_tx: Sender<Vec<u8>>) -> Result<(), WindowError> {
        if self.frames.len() == self.get_max_size() {
            return Err(WindowError::Full);
        } else {
            // Run a timer to resend the frame if it is not received
            let handle = create_frame_timer(frame.to_bytes(), writer_tx);
            self.frames.push_back((frame, handle));
        }

        Ok(())
    }

    /// Pop a frame from the front of the window and return it
    ///
    /// Returns `None` if the window is empty
    pub fn pop_front(&mut self, condition: SafeCond) -> Option<Frame> {
        let popped = self.frames.pop_front();

        if let Some(popped) = popped {
            // Abort the timer task for the frame that was popped
            popped.1.abort();

            // Notify the send task that space was created in the window
            condition.notify_one();

            return Some(popped.0);
        }

        None
    }

    /// Check if the window is full
    pub fn is_full(&self) -> bool {
        self.frames.len() == self.get_max_size()
    }

    /// Check if the window contains a frame with the given number
    pub fn contains(&self, num: u8) -> bool {
        self.frames.iter().any(|(frame, _)| frame.num == num)
    }

    /// Check if the window is empty
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Pop a specific frame from the window.
    ///
    /// # Arguments
    /// - `num`: The number of the frame to pop
    /// - `condition`: The condition variable to notify the send task
    ///
    /// # Returns
    /// The frame that was popped or `None` if the frame was not found
    pub fn pop(&mut self, num: u8, condition: SafeCond) -> Option<Frame> {
        let i = self.frames.iter().position(|(frame, _)| frame.num == num);
        match i {
            Some(i) => {
                let popped = self
                    .frames
                    .remove(i)
                    .expect("Frame not found, this should never happen");

                // Abort the timer task for the frame that was popped
                popped.1.abort();
                // Notify the send task that space was created in the window
                condition.notify_one();
                Some(popped.0)
            }
            None => None,
        }
    }

    /// Pop frames from the front of the window until the frame with the given number is reached.
    ///
    /// # Arguments
    /// - `num`: The number of the frame to pop until
    /// - `inclusive`: If true, the frame with the given number is also popped
    /// - `condition`: The condition variable to notify the send task
    ///
    /// Returns the number of frames popped
    pub fn pop_until(&mut self, num: u8, inclusive: bool, condition: SafeCond) -> usize {
        // Get the index of "limit" frame in the window
        let i = match self.frames.iter().position(|(frame, _)| frame.num == num) {
            Some(i) => i,
            None => {
                debug!("Frame not found in window, this means it was already acknowledged");
                return 0;
            }
        };

        // Pop the frames that were acknowledged
        let drained = if inclusive {
            self.frames.drain(..i + 1)
        } else {
            self.frames.drain(..i)
        };

        let drained_size = drained.len();

        // Abort the timers tasks for the frames that were popped
        drained.for_each(|(_, handle)| handle.abort());

        // Notify the send task that space was created in the window
        condition.notify_one();

        // Return the number of frames popped
        drained_size
    }
}

impl Default for Window {
    fn default() -> Self {
        Self::new()
    }
}

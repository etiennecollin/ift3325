use log::debug;

use crate::frame::Frame;
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
};

pub type SafeWindow = Arc<Mutex<Window>>;
pub type SafeCond = Arc<Condvar>;

// Define the error type for the window
#[derive(Debug)]
pub enum WindowError {
    /// The window is full and cannot accept any more frames
    Full,
}

/// A sliding window for a Go-Back-N protocol.
/// It implements a deque with a maximum size of `WINDOW_SIZE`.
pub struct Window {
    pub frames: VecDeque<Frame>,
    pub resend_all: bool,
    pub connected: bool,
    pub srej: bool,
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
            connected: false,
            srej: false,
        }
    }

    /// Get the maximum number of frames that can be in the window
    pub fn get_size(&self) -> usize {
        if self.srej {
            Self::SIZE_SREJ
        } else {
            Self::SIZE_GO_BACK_N
        }
    }

    /// Push a frame to the back of the window
    pub fn push(&mut self, frame: Frame) -> Result<(), WindowError> {
        if self.frames.len() == self.get_size() {
            return Err(WindowError::Full);
        } else {
            self.frames.push_back(frame);
        }

        Ok(())
    }

    /// Pop a frame from the front of the window and return it
    ///
    /// Returns `None` if the window is empty
    pub fn pop_front(&mut self, condition: &SafeCond) -> Option<Frame> {
        let popped = self.frames.pop_front();

        // Notify the send task that space was created in the window
        condition.notify_one();

        popped
    }

    /// Check if the window is full
    pub fn is_full(&self) -> bool {
        self.frames.len() == self.get_size()
    }

    /// Check if the window contains a frame with the given number
    pub fn contains(&self, num: u8) -> bool {
        self.frames.iter().any(|frame| frame.num == num)
    }

    /// Check if the window is empty
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Pop frames from the front of the window until the frame with the given number is reached
    pub fn pop_until(&mut self, num: u8, inclusive: bool, condition: &SafeCond) -> usize {
        let initial_len = self.frames.len();

        // Get the index of "limit" frame in the window
        let i = match self.frames.iter().position(|frame| frame.num == num) {
            Some(i) => i,
            None => {
                debug!("Frame not found in window, this means it was already acknowledged");
                return 0;
            }
        };

        // Pop the frames that were acknowledged
        if inclusive {
            self.frames.drain(..i + 1);
        } else {
            self.frames.drain(..i);
        }

        // Notify the send task that space was created in the window
        condition.notify_one();

        let final_len = self.frames.len();

        initial_len - final_len
    }
}

impl Default for Window {
    fn default() -> Self {
        Self::new()
    }
}

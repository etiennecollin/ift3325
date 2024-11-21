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
}

impl Window {
    /// Number of bits used for numbering frames
    pub const NUMBERING_BITS: usize = 3;
    /// The maximum number of frames that can be in the window for a Go-Back-N protocol
    pub const SIZE: usize = (1 << Self::NUMBERING_BITS) - 1;
    /// The maximum time in seconts to wait before a fame is considered lost
    pub const FRAME_TIMEOUT: u64 = 3;

    /// Create a new window
    /// The window is initially empty and has a capacity of `WINDOW_SIZE`
    pub fn new() -> Self {
        Self {
            frames: VecDeque::with_capacity(Self::SIZE),
            resend_all: false,
        }
    }

    /// Push a frame to the back of the window
    pub fn push(&mut self, frame: Frame) -> Result<(), WindowError> {
        if self.frames.len() == Self::SIZE {
            return Err(WindowError::Full);
        } else {
            self.frames.push_back(frame);
        }

        Ok(())
    }

    /// Check if the window is full
    pub fn isfull(&self) -> bool {
        self.frames.len() == Self::SIZE
    }

    /// Check if the window is empty
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Pop frames from the front of the window until the frame with the given number is reached
    pub fn pop_until(&mut self, num: u8) -> usize {
        let initial_len = self.frames.len();

        // Get the index of "limit" frame in the window
        let i = self
            .frames
            .iter()
            .position(|frame| frame.num == num)
            .expect("Frame not found in window, this should never happen");

        // Pop the frames that were acknowledged
        self.frames.drain(..i + 1);

        let final_len = self.frames.len();

        initial_len - final_len
    }
}

impl Default for Window {
    fn default() -> Self {
        Self::new()
    }
}

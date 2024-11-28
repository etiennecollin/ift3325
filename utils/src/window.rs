//! A sliding window for the HDLC protocol.

use crate::frame::Frame;
use log::debug;
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
};

/// Type alias for a safe window that can be shared and mutated between threads
pub type SafeWindow = Arc<Mutex<Window>>;
/// Type alias for a condition variable that can be safely shared between threads
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
    /// The frames in the window
    pub frames: VecDeque<Frame>,
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

    /// Push a frame to the back of the window
    /// If the window is full, an error is returned
    pub fn push(&mut self, frame: Frame) -> Result<(), WindowError> {
        if self.frames.len() == self.get_max_size() {
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
        self.frames.len() == self.get_max_size()
    }

    /// Check if the window contains a frame with the given number
    pub fn contains(&self, num: u8) -> bool {
        self.frames.iter().any(|frame| frame.num == num)
    }

    /// Check if the window is empty
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Pop frames from the front of the window until the frame with the given number is reached.
    ///
    /// # Arguments
    /// - `num`: The number of the frame to pop until
    /// - `inclusive`: If true, the frame with the given number is also popped
    /// - `condition`: The condition variable to notify the send task
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, FrameType};

    #[test]
    fn test_push_and_pop() {
        let mut window = Window::new();
        let frame = Frame::new(FrameType::Information, 0, vec![1, 2, 3]);
        assert!(window.push(frame).is_ok());
        assert_eq!(window.frames.len(), 1);

        let popped_frame = window.pop_front(&Arc::new(Condvar::new())).unwrap();
        assert_eq!(popped_frame.data, vec![1, 2, 3]);
    }

    #[test]
    fn test_window_full() {
        let mut window = Window::new();
        for i in 0..window.get_max_size() {
            assert!(window
                .push(Frame::new(FrameType::Information, i as u8, vec![]))
                .is_ok());
        }

        assert!(window.is_full());
        assert!(window
            .push(Frame::new(FrameType::Information, 0, vec![]))
            .is_err());
    }

    #[test]
    fn test_contains() {
        let mut window = Window::new();
        let frame = Frame::new(FrameType::Information, 1, vec![]);
        assert!(window.push(frame).is_ok());
        assert!(window.contains(1));
        assert!(!window.contains(2));
    }

    #[test]
    fn test_pop_until() {
        let mut window = Window::new();
        let cond = Arc::new(Condvar::new());
        for i in 0..3 {
            window
                .push(Frame::new(FrameType::Information, i as u8, vec![]))
                .unwrap();
        }

        assert_eq!(window.pop_until(1, true, &cond), 2);
        assert_eq!(window.frames.len(), 1);
    }
}

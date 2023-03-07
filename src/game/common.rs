use crate::game::{Revision, State};
use std::mem::size_of;
use std::time::Duration;
use zerocopy::FromBytes;

#[derive(Debug, FromBytes)]
#[repr(C)]
pub(super) struct GameObject {
    room_x: u32,
    room_y: u32,
    state: u32,
    gamestate: u32,
    timer: Timer<u32>,
}

impl GameObject {
    pub(super) fn into_state(self) -> (State, Duration) {
        log::trace!("{:?}", self);
        (
            State {
                room: (self.room_x, self.room_y),
                gamestate: self.gamestate,
                state: self.state,
            },
            self.timer.into(),
        )
    }
}
impl GameObject {
    /// parse revision from bytes
    ///
    /// panicks if `Some(bytes.len()) != revision.game_object_size()`.
    pub(super) fn from_bytes(revision: &Revision, bytes: &[u8]) -> Self {
        fn read_object<T: FromBytes>(bytes: &[u8], offset: usize) -> T {
            T::read_from(&bytes[offset..offset + size_of::<T>()]).expect("bad game object")
        }
        assert_eq!(bytes.len(), revision.game_object_size());
        Self {
            timer: read_object(bytes, revision.timer_offset),
            gamestate: read_object(bytes, revision.gamestate_offset),
            room_x: read_object(bytes, revision.room_x_offset),
            room_y: read_object(bytes, revision.room_y_offset),
            state: read_object(bytes, revision.state_offset),
        }
    }
}

#[derive(Debug, FromBytes)]
struct Timer<T> {
    frames: T,
    seconds: T,
    minutes: T,
    hours: T,
}

impl<T> From<Timer<T>> for Duration
where
    u64: From<T>,
    u32: From<T>,
{
    fn from(timer: Timer<T>) -> Duration {
        Duration::new(
            u64::from(timer.hours) * 3600
                + u64::from(timer.minutes) * 60
                + u64::from(timer.seconds),
            1_000_000_000u32 / 30 * u32::from(timer.frames),
        )
    }
}

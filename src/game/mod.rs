mod common;
mod linux;
mod macos;
mod revisions;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;

use anyhow::Result;
use debug_ignore::DebugIgnore;
use read_process_memory::Pid;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::sync::Once;
use std::time::Duration;

pub struct Revision {
    game_object_size: usize,
    room_x_offset: usize,
    room_y_offset: usize,
    state_offset: usize,
    gamestate_offset: usize,
    timer_offset: usize,
    playing_states: Vec<u32>,
}

impl Revision {
    pub fn get(name: &str) -> Option<&'static Self> {
        // ugh rust doesnt support const HashMaps or Vecs
        static mut REVISIONS: Option<HashMap<&'static str, Revision>> = None;
        static REVISIONS_ONCE: Once = Once::new();
        REVISIONS_ONCE.call_once(|| {
            // SAFETY: we only access REVISIONS here and below
            unsafe { REVISIONS = Some(revisions::get()) }
        });
        // SAFETY: we only modify REVISIONS in the Once above
        unsafe { REVISIONS.as_ref() }.unwrap().get(name)
    }

    pub(super) fn game_object_size(&self) -> usize {
        self.game_object_size
    }
}

impl Revision {
    fn is_playing_state(&self, state: u32) -> bool {
        self.playing_states.contains(&state)
    }
}

const SPLITS: [(Event, RangeInclusive<u32>); 8] = [
    (Event::Verdigris, 3006..=3011),
    (Event::Vermilion, 3060..=3065),
    (Event::Victoria, 3040..=3045),
    (Event::Violet, 4091..=4099),
    (Event::Vitellary, 3020..=3025),
    (Event::IntermissionOne, 3085..=3087),
    (Event::IntermissionTwo, 3080..=3082),
    (Event::GameComplete, 3503..=3509),
];

#[derive(Debug)]
pub(crate) struct Game {
    handle: DebugIgnore<imp::Handle>,
    old: State,
    cur: State,
}

#[derive(Debug, Clone, PartialEq)]
struct State {
    room: (u32, u32),
    gamestate: u32,
    state: u32,
}

impl State {
    fn new() -> State {
        State {
            room: (u32::MAX, u32::MAX),
            gamestate: u32::MAX,
            state: u32::MAX,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Update {
    pub(crate) time: Duration,
    pub(crate) event: Option<Event>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Event {
    NewGame,
    Verdigris,
    Vermilion,
    Victoria,
    Violet,
    Vitellary,
    IntermissionOne,
    IntermissionTwo,
    GameComplete,
    Reset,
}

impl Game {
    pub(crate) fn attach(pid: Pid) -> Result<Game> {
        let handle = imp::find_game_object(pid)?;
        log::info!("attached to pid {}", pid);
        Ok(Game {
            handle: DebugIgnore(handle),
            old: State::new(),
            cur: State::new(),
        })
    }

    pub(crate) fn update(&mut self, revision: &Revision) -> Result<Update> {
        let (state, time) = imp::read_game_object(&self.handle, revision)?;
        if self.old.state == u32::MAX {
            self.old = state.clone();
            self.cur = state;
        } else {
            self.old = std::mem::replace(&mut self.cur, state);
        }

        if self.old.room != self.cur.room {
            log::debug!(
                "room: {:?} -> {:?} @ {:?}",
                self.old.room,
                self.cur.room,
                time
            );
        }
        if self.old.gamestate != self.cur.gamestate {
            log::debug!(
                "gamestate: {} -> {} @ {:?}",
                self.old.gamestate,
                self.cur.gamestate,
                time
            );
        }
        if self.old.state != self.cur.state {
            log::debug!(
                "state: {} -> {} @ {:?}",
                self.old.state,
                self.cur.state,
                time
            );
        }

        if revision.is_playing_state(self.cur.gamestate)
            && !revision.is_playing_state(self.old.gamestate)
        {
            return Ok(Update {
                time: Duration::ZERO,
                event: Some(Event::NewGame),
            });
        }
        if !revision.is_playing_state(self.cur.gamestate)
            && revision.is_playing_state(self.old.gamestate)
        {
            return Ok(Update {
                time,
                event: Some(Event::Reset),
            });
        }
        
        // `state` increments to 3006 prior to the switch case that jumps to the correct state. This
        // can cause `Event::Verdigris` to fire one cycle before the correct event. Check we're in
        // the right room ("Murdering Twinmaker" @ (115, 100)) or (Untitled @ (113, 102)) (telejump)
        // and enforce no event if we're not.
        let event =
            if self.cur.state == 3006 && self.cur.room != (115, 100) && self.cur.room != (113, 102)
            {
                log::debug!("ignoring state 3006");
                None
            } else {
                SPLITS.into_iter().find_map(|(event, range)| {
                    (range.contains(&self.cur.state) && !range.contains(&self.old.state))
                        .then_some(event)
                })
            };

        Ok(Update { time, event })
    }
}

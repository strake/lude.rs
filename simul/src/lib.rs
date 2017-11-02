#![no_std]

//! Simulation with fixed tick and interpolative rendering
//!
//! # Example
//!
//! ```
//! let simulator = Simulator::new(tick, state.clone());
//! while let frame = simulator.go() {
//!     state.modify_with_user_input(input());
//!     draw(frame.simulate(&mut state, step, lerp));
//! }

extern crate clone;
extern crate idem;
extern crate time;

use clone::TryClone;
use core::ops::*;
use idem::Zero;

pub struct Simulator<State> {
    tick: time::Span,
    then: time::Point,
    cumul: time::Span,
    total: time::Span,
    prior_state: State,
}

impl<State> Simulator<State> {
    /// Make a `Simulator` with given simulational `tick` and original `state`.
    /// The original `state` is never used, except to call `try_clone_from`.
    /// Nonetheless, it must be given for the program to be well-defined.
    #[inline]
    pub fn new(tick: time::Span, state: State) -> Self { Simulator {
        tick: tick,
        then: time::Point::now(),
        cumul: Zero::zero,
        total: Zero::zero,
        prior_state: state,
    } }

    /// Return a `Frame` value which can be used to simulate the frame.
    /// The `Frame` remembers when it was created, so intervening code should not
    /// upset the simulation.
    #[inline]
    pub fn go(&mut self) -> Frame<State> { Frame(self, time::Point::now()) }

    /// Return the total elapsed time of simulation, including partial ticks.
    #[inline]
    pub fn total_time(&self) -> time::Span { self.total }

    #[inline]
    fn elapse(&mut self, now: time::Point) {
        let elapsed = now - self.then;
        debug_assert!(elapsed >= Zero::zero);
        self.then = now;
        self.cumul += elapsed;
        self.total += elapsed;
    }
}

pub struct Frame<'a, State: 'a>(&'a mut Simulator<State>, time::Point);

impl<'a, State: TryClone> Frame<'a, State> {
    /// Simulate the frame:
    ///
    /// * compute how much time passed since last frame
    /// * add any remaining accumulated unsimulated time
    /// * call `step` for each discrete tick-sized chunk
    /// * call `f` to interpolate up to the remaining partial tick (which may be zero)
    #[inline]
    pub fn simulate<A, Step, F>(mut self, state: &mut State,
                                step: Step, f: F) -> Result<A, State::Error>
      where Step: Fn(&mut State), F: FnOnce(f32, &State, &State) -> A {
        self.0.elapse(self.1);
        while self.cumul > Zero::zero {
            self.cumul -= self.tick;
            if self.cumul < Zero::zero {
                self.prior_state.try_clone_from(state)?;
            }
            step(state);
        }
        Ok(f((self.cumul.to_ns() as f32 / self.tick.to_ns() as f32).neg(),
             &state, if self.cumul == Zero::zero { state } else { &self.prior_state }))
    }

    /// Return when the `Frame` was created.
    /// This may be useful, for example, to compute how long to sleep after processing + drawing.
    #[inline]
    pub fn now(&self) -> time::Point { self.1 }
}

impl<'a, State> Deref for Frame<'a, State> {
    type Target = Simulator<State>;

    #[inline]
    fn deref(&self) -> &Simulator<State> { self.0 }
}

impl<'a, State> DerefMut for Frame<'a, State> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Simulator<State> { self.0 }
}

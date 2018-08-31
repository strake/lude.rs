#![no_std]

//! Simulation with fixed tick and interpolative rendering
//!
//! # Example
//!
//! ```ignore
//! let simulator = Simulator::new(tick);
//! while let frame = simulator.go() {
//!     state.modify_with_user_input(input());
//!     draw(frame.simulate(&mut state, step,
//!                         |a| a.part_needed_to_draw(),
//!                         |b| b,
//!                         |a| a.part_needed_to_draw().try_clone(),
//!                         lerp));
//! }

extern crate clone;
extern crate idem;
extern crate slot;
extern crate time;

use core::ops::*;
use idem::Zero;
use slot::Slot;

pub struct Simulator {
    tick: time::Span,
    then: time::Point,
    cumul: time::Span,
    total: time::Span,
}

impl Simulator {
    /// Make a `Simulator` with given simulational `tick`.
    #[inline]
    pub fn new(tick: time::Span) -> Self { Simulator {
        tick: tick,
        then: time::Point::now(),
        cumul: Zero::zero,
        total: Zero::zero,
    } }

    /// Return a `Frame` value which can be used to simulate the frame.
    /// The `Frame` remembers when it was created, so intervening code should not
    /// upset the simulation.
    #[inline]
    pub fn go<F: FnOnce(time::Span)>(&mut self, f: F) -> Frame<F> { Frame(self, time::Point::now(), Slot { x: f }) }

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

pub struct Frame<'a, F: FnOnce(time::Span)>(&'a mut Simulator, time::Point, Slot<F>);

impl<'a, _F: FnOnce(time::Span)> Frame<'a, _F> {
    /// Simulate the frame:
    ///
    /// * compute how much time passed since last frame
    /// * add any remaining accumulated unsimulated time
    /// * call `step` for each discrete tick-sized chunk
    /// * call `f` to interpolate up to the remaining partial tick (which may be zero)
    #[inline]
    pub fn simulate<A, B, C, D, E, F, G, H, I,
                    Step>(&mut self, state: &mut A, step: Step,
                          f: F, g: G, h: H, i: I) -> Result<D, E>
      where Step: Fn(&mut A), F: Fn(&A) -> C, G: Fn(&B) -> C,
            H: Fn(&A) -> Result<Option<B>, E>, I: FnOnce(f32, C, C) -> D {
        let mut prior_state_opt = None;
        self.0.elapse(self.1);
        while self.cumul > Zero::zero {
            self.cumul -= self.tick;
            if self.cumul < Zero::zero { prior_state_opt = h(state)?; }
            step(state);
        }
        Ok(i((self.cumul.to_ns() as f32 / self.tick.to_ns() as f32).neg(),
             f(state), prior_state_opt.as_ref().map_or(f(state), g)))
    }

    /// Return when the `Frame` was created.
    /// This may be useful, for example, to compute how long to sleep after processing + drawing.
    #[inline]
    pub fn now(&self) -> time::Point { self.1 }
}

impl<'a, F: FnOnce(time::Span)> Deref for Frame<'a, F> {
    type Target = Simulator;

    #[inline]
    fn deref(&self) -> &Simulator { self.0 }
}

impl<'a, F: FnOnce(time::Span)> DerefMut for Frame<'a, F> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Simulator { self.0 }
}

impl<'a, F: FnOnce(time::Span)> Drop for Frame<'a, F> {
    #[inline]
    fn drop(&mut self) { (unsafe { self.2.clone().unwrap() })(time::Point::now() - self.1) }
}

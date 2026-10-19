//! BashCutPro batch runner (#45; design notes in `bashcut/batch.md`).
//!
//! A pipeline (`bashcut-batch/1`, [`pipeline`]) lists inputs, how each item's project starts,
//! steps (engine commands, plugin actions, agent skills) and outputs. [`Job::create`] turns it into
//! a job book with one item, and one project, per input; [`run`] works through the items in
//! parallel, each step in its own `filmcraft-cli` process, and can stop, be cancelled and resume
//! (see [`book`] for the tripwire). No workspace dependencies: it drives the CLI, not the engine.

mod agent;
pub mod book;
pub mod governor;
pub mod pipeline;
pub mod presets;
mod process;
mod python;
pub mod runner;

pub use book::{AgentTotals, AgentUsage, Book, Category, Counts, ErrorInfo, ItemState, Job, Status, StepState, list_jobs};
pub use governor::{FakeProbe, Limits, Probe, Report, SystemProbe};
pub use pipeline::{Pipeline, SCHEMA};
pub use process::pid_alive;
pub use runner::{Options, Summary, run};

//! The interface each usage source implements. Providers run on their own
//! worker thread, so every method may block on the network.

use std::time::Duration;

use serde_json::Value;

use crate::model::{ProviderId, Snapshot};

pub enum FetchError {
    /// The session is gone; the user must sign in again.
    SignedOut,
    Other(String),
}

impl From<String> for FetchError {
    fn from(e: String) -> Self {
        FetchError::Other(e)
    }
}

pub enum PollResult {
    Pending,
    /// Credentials to hand to `Provider::accept_login`.
    Done(Value),
    Failed(String),
}

/// An in-progress browser sign-in.
pub trait Login {
    /// Page to open in the browser.
    fn url(&self) -> &str;
    /// A code the user must type into that page, if the flow needs one.
    fn user_code(&self) -> Option<&str> {
        None
    }
    fn poll_interval(&self) -> Duration;
    fn poll(&mut self) -> PollResult;
}

pub trait Provider: Send {
    fn id(&self) -> ProviderId;
    /// Loads stored credentials; returns true if signed in.
    fn load(&mut self) -> bool;
    fn begin_login(&mut self) -> Result<Box<dyn Login>, String>;
    /// Stores credentials from a finished login.
    fn accept_login(&mut self, creds: Value) -> Result<(), String>;
    fn sign_out(&mut self);
    /// Fetches usage. May call `send` more than once as detail arrives.
    fn fetch(&mut self, send: &mut dyn FnMut(Snapshot)) -> Result<(), FetchError>;
    /// Minimum time between polls.
    fn poll_every(&self) -> Duration {
        Duration::from_secs(5 * 60)
    }
}

#![no_std]
#[macro_use]
extern crate sgx_tstd as std;

mod client;
mod error;
mod quote;

pub use crate::client::AttestedReport;
pub use crate::error::FrameRAError as Error;
pub use crate::quote::{EncodedQuote, QuoteTarget};

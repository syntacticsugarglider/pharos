// See: https://github.com/rust-lang/rust/issues/44732#issuecomment-488766871
//
#![cfg_attr(feature = "external_doc", feature(external_doc))]
#![cfg_attr(feature = "external_doc", doc(include = "../README.md"))]
//!

#![doc(html_root_url = "https://docs.rs/pharos")]
#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![allow(clippy::suspicious_else_formatting)]
#![warn(
    missing_debug_implementations,
    missing_docs,
    nonstandard_style,
    rust_2018_idioms,
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_qualifications,
    single_use_lifetimes,
    unreachable_pub,
    variant_size_differences
)]

mod error;
mod events;
mod filter;
mod observable;
mod pharos;

pub use {
    self::pharos::Pharos,
    error::{Error, ErrorKind},
    events::Events,
    filter::Filter,
    observable::{Channel, Observable, ObserveConfig},
};

mod import {
    pub(crate) use {
        futures::{ready, Sink, Stream},
        futures_channel::mpsc::{
            self, Receiver as FutReceiver, SendError as FutSendError, Sender as FutSender,
            UnboundedReceiver as FutUnboundedReceiver, UnboundedSender as FutUnboundedSender,
        },
        std::{any::type_name, error::Error as ErrorTrait, fmt, ops::Deref},
        std::{
            pin::Pin,
            task::{Context, Poll},
        },
    };

    #[cfg(test)]
    //
    pub(crate) use {
        assert_matches::assert_matches,
        futures::{executor::block_on, future::poll_fn, SinkExt},
    };
}

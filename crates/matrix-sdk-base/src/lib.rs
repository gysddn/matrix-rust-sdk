// Copyright 2020 Damir Jelić
// Copyright 2020 The Matrix.org Foundation C.I.C.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![doc = include_str!("../README.md")]
#![cfg_attr(feature = "docs", feature(doc_cfg))]
#![deny(
    missing_debug_implementations,
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications
)]

pub use matrix_sdk_common::*;

pub use crate::{
    error::{Error, Result},
    session::Session,
};

mod client;
mod error;
pub mod media;
mod rooms;
mod session;
mod store;

pub use client::{BaseClient, BaseClientConfig};
#[cfg(feature = "encryption")]
#[cfg_attr(feature = "docs", doc(cfg(encryption)))]
pub use matrix_sdk_crypto as crypto;
pub use rooms::{Room, RoomInfo, RoomMember, RoomType};
pub use store::{StateChanges, StateStore, Store, StoreError};

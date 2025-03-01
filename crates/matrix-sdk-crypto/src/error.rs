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

use olm_rs::errors::{OlmGroupSessionError, OlmSessionError};
use ruma::{identifiers::Error as IdentifierError, DeviceId, RoomId, UserId};
use serde_json::Error as SerdeError;
use thiserror::Error;

use super::store::CryptoStoreError;

pub type OlmResult<T> = Result<T, OlmError>;
pub type MegolmResult<T> = Result<T, MegolmError>;

/// Error representing a failure during a device to device cryptographic
/// operation.
#[derive(Error, Debug)]
pub enum OlmError {
    /// The event that should have been decrypted is malformed.
    #[error(transparent)]
    EventError(#[from] EventError),

    /// The received decrypted event couldn't be deserialized.
    #[error(transparent)]
    JsonError(#[from] SerdeError),

    /// The underlying Olm session operation returned an error.
    #[error("can't finish Olm Session operation {0}")]
    OlmSession(#[from] OlmSessionError),

    /// The underlying group session operation returned an error.
    #[error("can't finish Olm Session operation {0}")]
    OlmGroupSession(#[from] OlmGroupSessionError),

    /// The storage layer returned an error.
    #[error("failed to read or write to the crypto store {0}")]
    Store(#[from] CryptoStoreError),

    /// The session with a device has become corrupted.
    #[error(
        "decryption failed likely because an Olm session from {0} with sender key {1} was wedged"
    )]
    SessionWedged(UserId, String),

    /// An Olm message got replayed while the Olm ratchet has already moved
    /// forward.
    #[error("decryption failed because an Olm message from {0} with sender key {1} was replayed")]
    ReplayedMessage(UserId, String),

    /// Encryption failed because the device does not have a valid Olm session
    /// with us.
    #[error(
        "encryption failed because the device does not \
            have a valid Olm session with us"
    )]
    MissingSession,
}

/// Error representing a failure during a group encryption operation.
#[derive(Error, Debug)]
pub enum MegolmError {
    /// The event that should have been decrypted is malformed.
    #[error(transparent)]
    EventError(#[from] EventError),

    /// The received decrypted event couldn't be deserialized.
    #[error(transparent)]
    JsonError(#[from] SerdeError),

    /// Decryption failed because the session needed to decrypt the event is
    /// missing.
    #[error("decryption failed because the session to decrypt the message is missing")]
    MissingSession,

    /// The underlying group session operation returned an error.
    #[error("can't finish Olm group session operation {0}")]
    OlmGroupSession(#[from] OlmGroupSessionError),

    /// The room where a group session should be shared is not encrypted.
    #[error("The room where a group session should be shared is not encrypted")]
    EncryptionNotEnabled,

    /// The storage layer returned an error.
    #[error(transparent)]
    Store(#[from] CryptoStoreError),
}

#[derive(Error, Debug)]
pub enum EventError {
    #[error("the Olm message has a unsupported type")]
    UnsupportedOlmType,

    #[error("the Encrypted message has been encrypted with a unsupported algorithm.")]
    UnsupportedAlgorithm,

    #[error("the provided JSON value isn't an object")]
    NotAnObject,

    #[error("the Encrypted message doesn't contain a ciphertext for our device")]
    MissingCiphertext,

    #[error("the Encrypted message is missing the signing key of the sender")]
    MissingSigningKey,

    #[error("the Encrypted message is missing the sender key")]
    MissingSenderKey,

    #[error("the Encrypted message is missing the field {0}")]
    MissingField(String),

    #[error("the sender of the plaintext doesn't match the sender of the encrypted message.")]
    MismatchedSender,

    #[error("the keys of the message don't match the keys in our database.")]
    MismatchedKeys,

    #[error(
        "the room id of the room key doesn't match the room id of the \
        decrypted event: expected {0}, got {:1}"
    )]
    MismatchedRoom(RoomId, Option<RoomId>),
}

#[derive(Error, Debug)]
pub enum SessionUnpicklingError {
    /// The underlying Olm session operation returned an error.
    #[error("can't finish Olm Session operation {0}")]
    OlmSession(#[from] OlmSessionError),
    /// The Session timestamp was invalid.
    #[error("can't load session timestamps")]
    SessionTimestampError,
}

/// Error type describin different errors that happen when we check or create
/// signatures for a Matrix JSON object.
#[derive(Error, Debug)]
pub enum SignatureError {
    /// The signature was made using an unsupported algorithm.
    #[error("the signature used an unsupported algorithm")]
    UnsupportedAlgorithm,

    /// The ID of the signing key isn't a valid key ID.
    #[error("the ID of the signing key is invalid")]
    InvalidKeyId(#[from] IdentifierError),

    /// The signing key that should create or check a signature is missing.
    #[error("the signing key is missing from the object that signed the message")]
    MissingSigningKey,

    /// The user id of signing key differs from the user id that provided the
    /// signature.
    #[error("the user id of the signing key differs user id that provided the signature")]
    UserIdMismatch,

    /// The provided JSON value that was signed and the signature should be
    /// checked isn't a valid JSON object.
    #[error("the provided JSON value isn't an object")]
    NotAnObject,

    /// The provided JSON value that was signed and the signature should be
    /// checked isn't a valid JSON object.
    #[error("the provided JSON object doesn't contain a signatures field")]
    NoSignatureFound,

    /// The signature couldn't be verified.
    #[error("the signature didn't match the provided key")]
    VerificationError,

    /// The signed object couldn't be deserialized.
    #[error(transparent)]
    JsonError(#[from] SerdeError),
}

#[derive(Error, Debug)]
pub(crate) enum SessionCreationError {
    #[error(
        "Failed to create a new Olm session for {0} {1}, the requested \
        one-time key isn't a signed curve key"
    )]
    OneTimeKeyNotSigned(UserId, Box<DeviceId>),
    #[error(
        "Tried to create a new Olm session for {0} {1}, but the signed \
        one-time key is missing"
    )]
    OneTimeKeyMissing(UserId, Box<DeviceId>),
    #[error(
        "Tried to create a new Olm session for {0} {1}, but the one-time \
        key algorithm is unsupported"
    )]
    OneTimeKeyUnknown(UserId, Box<DeviceId>),
    #[error("Failed to verify the one-time key signatures for {0} {1}: {2:?}")]
    InvalidSignature(UserId, Box<DeviceId>, SignatureError),
    #[error(
        "Tried to create an Olm session for {0} {1}, but the device is missing \
        a curve25519 key"
    )]
    DeviceMissingCurveKey(UserId, Box<DeviceId>),
    #[error("Error creating new Olm session for {0} {1}: {2:?}")]
    OlmError(UserId, Box<DeviceId>, OlmSessionError),
}

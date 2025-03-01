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

use std::{collections::BTreeMap, convert::TryFrom, fmt, mem, sync::Arc};

use matrix_sdk_common::locks::Mutex;
pub use olm_rs::{
    account::IdentityKeys,
    session::{OlmMessage, PreKeyMessage},
    utility::OlmUtility,
};
use olm_rs::{
    errors::OlmGroupSessionError, inbound_group_session::OlmInboundGroupSession, PicklingMode,
};
use ruma::{
    events::{
        forwarded_room_key::ToDeviceForwardedRoomKeyEventContent,
        room::{
            encrypted::{EncryptedEventScheme, SyncEncryptedEvent},
            history_visibility::HistoryVisibility,
        },
        AnySyncRoomEvent,
    },
    serde::Raw,
    DeviceKeyAlgorithm, EventEncryptionAlgorithm, RoomId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

use super::{ExportedGroupSessionKey, ExportedRoomKey, GroupSessionKey};
use crate::error::{EventError, MegolmResult};

// TODO add creation times to the inbound group sessions so we can export
// sessions that were created between some time period, this should only be set
// for non-imported sessions.

/// Inbound group session.
///
/// Inbound group sessions are used to exchange room messages between a group of
/// participants. Inbound group sessions are used to decrypt the room messages.
#[derive(Clone)]
pub struct InboundGroupSession {
    inner: Arc<Mutex<OlmInboundGroupSession>>,
    history_visibility: Arc<Option<HistoryVisibility>>,
    session_id: Arc<str>,
    first_known_index: u32,
    pub(crate) sender_key: Arc<str>,
    pub(crate) signing_keys: Arc<BTreeMap<DeviceKeyAlgorithm, String>>,
    pub(crate) room_id: Arc<RoomId>,
    forwarding_chains: Arc<Vec<String>>,
    imported: bool,
    backed_up: bool,
}

impl InboundGroupSession {
    /// Create a new inbound group session for the given room.
    ///
    /// These sessions are used to decrypt room messages.
    ///
    /// # Arguments
    ///
    /// * `sender_key` - The public curve25519 key of the account that
    /// sent us the session
    ///
    /// * `signing_key` - The public ed25519 key of the account that
    /// sent us the session.
    ///
    /// * `room_id` - The id of the room that the session is used in.
    ///
    /// * `session_key` - The private session key that is used to decrypt
    /// messages.
    pub(crate) fn new(
        sender_key: &str,
        signing_key: &str,
        room_id: &RoomId,
        session_key: GroupSessionKey,
        history_visibility: Option<HistoryVisibility>,
    ) -> Result<Self, OlmGroupSessionError> {
        let session = OlmInboundGroupSession::new(&session_key.0)?;
        let session_id = session.session_id();
        let first_known_index = session.first_known_index();

        let mut keys: BTreeMap<DeviceKeyAlgorithm, String> = BTreeMap::new();
        keys.insert(DeviceKeyAlgorithm::Ed25519, signing_key.to_owned());

        Ok(InboundGroupSession {
            inner: Arc::new(Mutex::new(session)),
            session_id: session_id.into(),
            history_visibility: history_visibility.into(),
            sender_key: sender_key.to_owned().into(),
            first_known_index,
            signing_keys: keys.into(),
            room_id: room_id.clone().into(),
            forwarding_chains: Vec::new().into(),
            imported: false,
            backed_up: false,
        })
    }

    /// Create a InboundGroupSession from an exported version of the group
    /// session.
    ///
    /// Most notably this can be called with an `ExportedRoomKey` from a
    /// previous [`export()`] call.
    ///
    ///
    /// [`export()`]: #method.export
    pub fn from_export(
        exported_session: impl Into<ExportedRoomKey>,
    ) -> Result<Self, OlmGroupSessionError> {
        Self::try_from(exported_session.into())
    }

    /// Create a new inbound group session from a forwarded room key content.
    ///
    /// # Arguments
    ///
    /// * `sender_key` - The public curve25519 key of the account that
    /// sent us the session
    ///
    /// * `content` - A forwarded room key content that contains the session key
    /// to create the `InboundGroupSession`.
    pub(crate) fn from_forwarded_key(
        sender_key: &str,
        content: &mut ToDeviceForwardedRoomKeyEventContent,
    ) -> Result<Self, OlmGroupSessionError> {
        let key = Zeroizing::from(mem::take(&mut content.session_key));

        let session = OlmInboundGroupSession::import(&key)?;
        let first_known_index = session.first_known_index();
        let mut forwarding_chains = content.forwarding_curve25519_key_chain.clone();
        forwarding_chains.push(sender_key.to_owned());

        let mut sender_claimed_key = BTreeMap::new();
        sender_claimed_key
            .insert(DeviceKeyAlgorithm::Ed25519, content.sender_claimed_ed25519_key.to_owned());

        Ok(InboundGroupSession {
            inner: Mutex::new(session).into(),
            session_id: content.session_id.as_str().into(),
            sender_key: content.sender_key.as_str().into(),
            first_known_index,
            history_visibility: None.into(),
            signing_keys: sender_claimed_key.into(),
            room_id: content.room_id.clone().into(),
            forwarding_chains: forwarding_chains.into(),
            imported: true,
            backed_up: false,
        })
    }

    /// Store the group session as a base64 encoded string.
    ///
    /// # Arguments
    ///
    /// * `pickle_mode` - The mode that was used to pickle the group session,
    /// either an unencrypted mode or an encrypted using passphrase.
    pub async fn pickle(&self, pickle_mode: PicklingMode) -> PickledInboundGroupSession {
        let pickle = self.inner.lock().await.pickle(pickle_mode);

        PickledInboundGroupSession {
            pickle: InboundGroupSessionPickle::from(pickle),
            sender_key: self.sender_key.to_string(),
            signing_key: (&*self.signing_keys).clone(),
            room_id: (&*self.room_id).clone(),
            forwarding_chains: self.forwarding_key_chain().to_vec(),
            imported: self.imported,
            backed_up: self.backed_up,
            history_visibility: self.history_visibility.as_ref().clone(),
        }
    }

    /// Export this session at the first known message index.
    ///
    /// If only a limited part of this session should be exported use
    /// [`export_at_index()`](#method.export_at_index).
    pub async fn export(&self) -> ExportedRoomKey {
        self.export_at_index(self.first_known_index()).await
    }

    /// Get the sender key that this session was received from.
    pub fn sender_key(&self) -> &str {
        &self.sender_key
    }

    /// Get the map of signing keys this session was received from.
    pub fn signing_keys(&self) -> &BTreeMap<DeviceKeyAlgorithm, String> {
        &self.signing_keys
    }

    /// Get the list of ed25519 keys that this session was forwarded through.
    ///
    /// Each ed25519 key represents a single device. If device A forwards the
    /// session to device B and device B to C this list will contain the ed25519
    /// keys of A and B.
    pub fn forwarding_key_chain(&self) -> &[String] {
        &self.forwarding_chains
    }

    /// Export this session at the given message index.
    pub async fn export_at_index(&self, message_index: u32) -> ExportedRoomKey {
        let message_index = std::cmp::max(self.first_known_index(), message_index);

        let session_key = ExportedGroupSessionKey(
            self.inner.lock().await.export(message_index).expect("Can't export session"),
        );

        ExportedRoomKey {
            algorithm: EventEncryptionAlgorithm::MegolmV1AesSha2,
            room_id: (&*self.room_id).clone(),
            sender_key: (&*self.sender_key).to_owned(),
            session_id: self.session_id().to_owned(),
            forwarding_curve25519_key_chain: self.forwarding_key_chain().to_vec(),
            sender_claimed_keys: (&*self.signing_keys).clone(),
            session_key,
        }
    }

    /// Restore a Session from a previously pickled string.
    ///
    /// Returns the restored group session or a `OlmGroupSessionError` if there
    /// was an error.
    ///
    /// # Arguments
    ///
    /// * `pickle` - The pickled version of the `InboundGroupSession`.
    ///
    /// * `pickle_mode` - The mode that was used to pickle the session, either
    /// an unencrypted mode or an encrypted using passphrase.
    pub fn from_pickle(
        pickle: PickledInboundGroupSession,
        pickle_mode: PicklingMode,
    ) -> Result<Self, OlmGroupSessionError> {
        let session = OlmInboundGroupSession::unpickle(pickle.pickle.0, pickle_mode)?;
        let first_known_index = session.first_known_index();
        let session_id = session.session_id();

        Ok(InboundGroupSession {
            inner: Mutex::new(session).into(),
            session_id: session_id.into(),
            sender_key: pickle.sender_key.into(),
            history_visibility: pickle.history_visibility.into(),
            first_known_index,
            signing_keys: pickle.signing_key.into(),
            room_id: pickle.room_id.into(),
            forwarding_chains: pickle.forwarding_chains.into(),
            backed_up: pickle.backed_up,
            imported: pickle.imported,
        })
    }

    /// The room where this session is used in.
    pub fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    /// Returns the unique identifier for this session.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the first message index we know how to decrypt.
    pub fn first_known_index(&self) -> u32 {
        self.first_known_index
    }

    /// Decrypt the given ciphertext.
    ///
    /// Returns the decrypted plaintext or an `OlmGroupSessionError` if
    /// decryption failed.
    ///
    /// # Arguments
    ///
    /// * `message` - The message that should be decrypted.
    pub(crate) async fn decrypt_helper(
        &self,
        message: String,
    ) -> Result<(String, u32), OlmGroupSessionError> {
        self.inner.lock().await.decrypt(message)
    }

    /// Decrypt an event from a room timeline.
    ///
    /// # Arguments
    ///
    /// * `event` - The event that should be decrypted.
    pub(crate) async fn decrypt(
        &self,
        event: &SyncEncryptedEvent,
    ) -> MegolmResult<(Raw<AnySyncRoomEvent>, u32)> {
        let content = match &event.content.scheme {
            EncryptedEventScheme::MegolmV1AesSha2(c) => c,
            _ => return Err(EventError::UnsupportedAlgorithm.into()),
        };

        let (plaintext, message_index) = self.decrypt_helper(content.ciphertext.clone()).await?;

        let mut decrypted_value = serde_json::from_str::<Value>(&plaintext)?;
        let decrypted_object = decrypted_value.as_object_mut().ok_or(EventError::NotAnObject)?;

        let server_ts: i64 = event.origin_server_ts.0.into();

        decrypted_object.insert("sender".to_owned(), event.sender.to_string().into());
        decrypted_object.insert("event_id".to_owned(), event.event_id.to_string().into());
        decrypted_object.insert("origin_server_ts".to_owned(), server_ts.into());

        let room_id = decrypted_object
            .get("room_id")
            .and_then(|r| r.as_str().and_then(|r| RoomId::try_from(r).ok()));

        // Check that we have a room id and that the event wasn't forwarded from
        // another room.
        if room_id.as_ref() != Some(self.room_id()) {
            return Err(EventError::MismatchedRoom(self.room_id().to_owned(), room_id).into());
        }

        decrypted_object.insert(
            "unsigned".to_owned(),
            serde_json::to_value(&event.unsigned).unwrap_or_default(),
        );

        if let Some(decrypted_content) =
            decrypted_object.get_mut("content").map(|c| c.as_object_mut()).flatten()
        {
            if !decrypted_content.contains_key("m.relates_to") {
                let content = serde_json::to_value(&event.content)?;
                if let Some(relation) = content.as_object().and_then(|o| o.get("m.relates_to")) {
                    decrypted_content.insert("m.relates_to".to_owned(), relation.to_owned());
                }
            }
        }

        Ok((serde_json::from_value::<Raw<AnySyncRoomEvent>>(decrypted_value)?, message_index))
    }
}

#[cfg(not(tarpaulin_include))]
impl fmt::Debug for InboundGroupSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InboundGroupSession").field("session_id", &self.session_id()).finish()
    }
}

impl PartialEq for InboundGroupSession {
    fn eq(&self, other: &Self) -> bool {
        self.session_id() == other.session_id()
    }
}

/// A pickled version of an `InboundGroupSession`.
///
/// Holds all the information that needs to be stored in a database to restore
/// an InboundGroupSession.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickledInboundGroupSession {
    /// The pickle string holding the InboundGroupSession.
    pub pickle: InboundGroupSessionPickle,
    /// The public curve25519 key of the account that sent us the session
    pub sender_key: String,
    /// The public ed25519 key of the account that sent us the session.
    pub signing_key: BTreeMap<DeviceKeyAlgorithm, String>,
    /// The id of the room that the session is used in.
    pub room_id: RoomId,
    /// The list of claimed ed25519 that forwarded us this key. Will be None if
    /// we directly received this session.
    #[serde(default)]
    pub forwarding_chains: Vec<String>,
    /// Flag remembering if the session was directly sent to us by the sender
    /// or if it was imported.
    pub imported: bool,
    /// Flag remembering if the session has been backed up.
    #[serde(default)]
    pub backed_up: bool,
    /// History visibility of the room when the session was created.
    pub history_visibility: Option<HistoryVisibility>,
}

/// The typed representation of a base64 encoded string of the GroupSession
/// pickle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundGroupSessionPickle(String);

impl From<String> for InboundGroupSessionPickle {
    fn from(pickle_string: String) -> Self {
        InboundGroupSessionPickle(pickle_string)
    }
}

impl InboundGroupSessionPickle {
    /// Get the string representation of the pickle.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<ExportedRoomKey> for InboundGroupSession {
    type Error = OlmGroupSessionError;

    fn try_from(key: ExportedRoomKey) -> Result<Self, Self::Error> {
        let session = OlmInboundGroupSession::import(&key.session_key.0)?;
        let first_known_index = session.first_known_index();

        Ok(InboundGroupSession {
            inner: Arc::new(Mutex::new(session)),
            session_id: key.session_id.into(),
            sender_key: key.sender_key.into(),
            history_visibility: None.into(),
            first_known_index,
            signing_keys: Arc::new(key.sender_claimed_keys),
            room_id: Arc::new(key.room_id),
            forwarding_chains: Arc::new(key.forwarding_curve25519_key_chain),
            imported: true,
            backed_up: false,
        })
    }
}

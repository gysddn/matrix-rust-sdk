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

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use dashmap::{DashMap, DashSet};
use matrix_sdk_common::uuid::Uuid;
use ruma::{
    api::client::r0::keys::claim_keys::{
        Request as KeysClaimRequest, Response as KeysClaimResponse,
    },
    assign,
    events::{dummy::ToDeviceDummyEventContent, AnyToDeviceEventContent},
    DeviceId, DeviceIdBox, DeviceKeyAlgorithm, UserId,
};
use tracing::{error, info, warn};

use crate::{
    error::OlmResult,
    gossiping::GossipMachine,
    olm::Account,
    requests::{OutgoingRequest, ToDeviceRequest},
    store::{Changes, Result as StoreResult, Store},
    ReadOnlyDevice,
};

#[derive(Debug, Clone)]
pub(crate) struct SessionManager {
    account: Account,
    store: Store,
    /// A map of user/devices that we need to automatically claim keys for.
    /// Submodules can insert user/device pairs into this map and the
    /// user/device paris will be added to the list of users when
    /// [`get_missing_sessions`](#method.get_missing_sessions) is called.
    users_for_key_claim: Arc<DashMap<UserId, DashSet<DeviceIdBox>>>,
    wedged_devices: Arc<DashMap<UserId, DashSet<DeviceIdBox>>>,
    key_request_machine: GossipMachine,
    outgoing_to_device_requests: Arc<DashMap<Uuid, OutgoingRequest>>,
}

impl SessionManager {
    const KEY_CLAIM_TIMEOUT: Duration = Duration::from_secs(10);
    const UNWEDGING_INTERVAL: Duration = Duration::from_secs(60 * 60);

    pub fn new(
        account: Account,
        users_for_key_claim: Arc<DashMap<UserId, DashSet<DeviceIdBox>>>,
        key_request_machine: GossipMachine,
        store: Store,
    ) -> Self {
        Self {
            account,
            store,
            key_request_machine,
            users_for_key_claim,
            wedged_devices: Default::default(),
            outgoing_to_device_requests: Default::default(),
        }
    }

    /// Mark the outgoing request as sent.
    pub fn mark_outgoing_request_as_sent(&self, id: &Uuid) {
        self.outgoing_to_device_requests.remove(id);
    }

    pub async fn mark_device_as_wedged(&self, sender: &UserId, curve_key: &str) -> StoreResult<()> {
        if let Some(device) = self.store.get_device_from_curve_key(sender, curve_key).await? {
            let sessions = device.get_sessions().await?;

            if let Some(sessions) = sessions {
                let mut sessions = sessions.lock().await;
                sessions.sort_by_key(|s| s.creation_time.clone());

                let session = sessions.get(0);

                if let Some(session) = session {
                    if session.creation_time.elapsed() > Self::UNWEDGING_INTERVAL {
                        self.users_for_key_claim
                            .entry(device.user_id().clone())
                            .or_insert_with(DashSet::new)
                            .insert(device.device_id().into());
                        self.wedged_devices
                            .entry(device.user_id().to_owned())
                            .or_insert_with(DashSet::new)
                            .insert(device.device_id().into());
                    }
                }
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn is_device_wedged(&self, device: &ReadOnlyDevice) -> bool {
        self.wedged_devices
            .get(device.user_id())
            .map(|d| d.contains(device.device_id()))
            .unwrap_or(false)
    }

    /// Check if the session was created to unwedge a Device.
    ///
    /// If the device was wedged this will queue up a dummy to-device message.
    async fn check_if_unwedged(&self, user_id: &UserId, device_id: &DeviceId) -> OlmResult<()> {
        if self.wedged_devices.get(user_id).map(|d| d.remove(device_id)).flatten().is_some() {
            if let Some(device) = self.store.get_device(user_id, device_id).await? {
                let content = AnyToDeviceEventContent::Dummy(ToDeviceDummyEventContent::new());
                let (_, content) = device.encrypt(content).await?;

                let request = ToDeviceRequest::new(
                    device.user_id(),
                    device.device_id().to_owned(),
                    AnyToDeviceEventContent::RoomEncrypted(content),
                );

                let request = OutgoingRequest {
                    request_id: request.txn_id,
                    request: Arc::new(request.into()),
                };

                self.outgoing_to_device_requests.insert(request.request_id, request);
            }
        }

        Ok(())
    }

    /// Get the a key claiming request for the user/device pairs that we are
    /// missing Olm sessions for.
    ///
    /// Returns None if no key claiming request needs to be sent out.
    ///
    /// Sessions need to be established between devices so group sessions for a
    /// room can be shared with them.
    ///
    /// This should be called every time a group session needs to be shared as
    /// well as between sync calls. After a sync some devices may request room
    /// keys without us having a valid Olm session with them, making it
    /// impossible to server the room key request, thus it's necessary to check
    /// for missing sessions between sync as well.
    ///
    /// **Note**: Care should be taken that only one such request at a time is
    /// in flight, e.g. using a lock.
    ///
    /// The response of a successful key claiming requests needs to be passed to
    /// the `OlmMachine` with the [`receive_keys_claim_response`].
    ///
    /// # Arguments
    ///
    /// `users` - The list of users that we should check if we lack a session
    /// with one of their devices. This can be an empty iterator when calling
    /// this method between sync requests.
    ///
    /// [`receive_keys_claim_response`]: #method.receive_keys_claim_response
    pub async fn get_missing_sessions(
        &self,
        users: impl Iterator<Item = &UserId>,
    ) -> StoreResult<Option<(Uuid, KeysClaimRequest)>> {
        let mut missing = BTreeMap::new();

        // Add the list of devices that the user wishes to establish sessions
        // right now.
        for user_id in users {
            let user_devices = self.store.get_readonly_devices(user_id).await?;

            for (device_id, device) in user_devices.into_iter() {
                let sender_key = if let Some(k) = device.get_key(DeviceKeyAlgorithm::Curve25519) {
                    k
                } else {
                    continue;
                };

                let sessions = self.store.get_sessions(sender_key).await?;

                let is_missing = if let Some(sessions) = sessions {
                    sessions.lock().await.is_empty()
                } else {
                    true
                };

                if is_missing {
                    missing
                        .entry(user_id.to_owned())
                        .or_insert_with(BTreeMap::new)
                        .insert(device_id, DeviceKeyAlgorithm::SignedCurve25519);
                }
            }
        }

        // Add the list of sessions that for some reason automatically need to
        // create an Olm session.
        for item in self.users_for_key_claim.iter() {
            let user = item.key();

            for device_id in item.value().iter() {
                missing
                    .entry(user.to_owned())
                    .or_insert_with(BTreeMap::new)
                    .insert(device_id.to_owned(), DeviceKeyAlgorithm::SignedCurve25519);
            }
        }

        if missing.is_empty() {
            Ok(None)
        } else {
            Ok(Some((
                Uuid::new_v4(),
                assign!(KeysClaimRequest::new(missing), {
                    timeout: Some(Self::KEY_CLAIM_TIMEOUT),
                }),
            )))
        }
    }

    /// Receive a successful key claim response and create new Olm sessions with
    /// the claimed keys.
    ///
    /// # Arguments
    ///
    /// * `response` - The response containing the claimed one-time keys.
    pub async fn receive_keys_claim_response(&self, response: &KeysClaimResponse) -> OlmResult<()> {
        // TODO log the failures here

        let mut changes = Changes::default();

        for (user_id, user_devices) in &response.one_time_keys {
            for (device_id, key_map) in user_devices {
                let device = match self.store.get_readonly_device(user_id, device_id).await {
                    Ok(Some(d)) => d,
                    Ok(None) => {
                        warn!(
                            "Tried to create an Olm session for {} {}, but the device is unknown",
                            user_id, device_id
                        );
                        continue;
                    }
                    Err(e) => {
                        warn!(
                            "Tried to create an Olm session for {} {}, but \
                            can't fetch the device from the store {:?}",
                            user_id, device_id, e
                        );
                        continue;
                    }
                };

                info!("Creating outbound Session for {} {}", user_id, device_id);

                let session = match self.account.create_outbound_session(device, key_map).await {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("Error creating new outbound session {:?}", e);
                        continue;
                    }
                };

                changes.sessions.push(session);

                self.key_request_machine.retry_keyshare(user_id, device_id);

                if let Err(e) = self.check_if_unwedged(user_id, device_id).await {
                    error!(
                        "Error while treating an unwedged device {} {} {:?}",
                        user_id, device_id, e
                    );
                }
            }
        }

        // TODO turn this into a single save_changes() call.
        self.store.save_changes(changes).await?;

        match self.key_request_machine.collect_incoming_key_requests().await {
            Ok(sessions) => {
                let changes = Changes { sessions, ..Default::default() };
                self.store.save_changes(changes).await?
            }
            // We don't propagate the error here since the next sync will retry
            // this.
            Err(e) => {
                warn!(error =? e, "Error while trying to collect the incoming secret requests")
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::{collections::BTreeMap, sync::Arc};

    use dashmap::DashMap;
    use matrix_sdk_common::locks::Mutex;
    use matrix_sdk_test::async_test;
    use ruma::{
        api::client::r0::keys::claim_keys::Response as KeyClaimResponse, user_id, DeviceIdBox,
        UserId,
    };

    use super::SessionManager;
    use crate::{
        gossiping::GossipMachine,
        identities::ReadOnlyDevice,
        olm::{Account, PrivateCrossSigningIdentity, ReadOnlyAccount},
        session_manager::GroupSessionCache,
        store::{CryptoStore, MemoryStore, Store},
        verification::VerificationMachine,
    };

    fn user_id() -> UserId {
        user_id!("@example:localhost")
    }

    fn device_id() -> DeviceIdBox {
        "DEVICEID".into()
    }

    fn bob_account() -> ReadOnlyAccount {
        ReadOnlyAccount::new(&user_id!("@bob:localhost"), "BOBDEVICE".into())
    }

    async fn session_manager() -> SessionManager {
        let user_id = user_id();
        let device_id = device_id();

        let users_for_key_claim = Arc::new(DashMap::new());
        let account = ReadOnlyAccount::new(&user_id, &device_id);
        let store: Arc<dyn CryptoStore> = Arc::new(MemoryStore::new());
        store.save_account(account.clone()).await.unwrap();
        let identity = Arc::new(Mutex::new(PrivateCrossSigningIdentity::empty(user_id.clone())));
        let verification =
            VerificationMachine::new(account.clone(), identity.clone(), store.clone());

        let user_id = Arc::new(user_id);
        let device_id = device_id.into();

        let store = Store::new(user_id.clone(), identity, store, verification);

        let account = Account { inner: account, store: store.clone() };

        let session_cache = GroupSessionCache::new(store.clone());

        let key_request = GossipMachine::new(
            user_id,
            device_id,
            store.clone(),
            session_cache,
            users_for_key_claim.clone(),
        );

        SessionManager::new(account, users_for_key_claim, key_request, store)
    }

    #[async_test]
    async fn session_creation() {
        let manager = session_manager().await;
        let bob = bob_account();

        let bob_device = ReadOnlyDevice::from_account(&bob).await;

        manager.store.save_devices(&[bob_device]).await.unwrap();

        let (_, request) = manager
            .get_missing_sessions(&mut [bob.user_id().clone()].iter())
            .await
            .unwrap()
            .unwrap();

        assert!(request.one_time_keys.contains_key(bob.user_id()));

        bob.generate_one_time_keys_helper(1).await;
        let one_time = bob.signed_one_time_keys_helper().await.unwrap();
        bob.mark_keys_as_published().await;

        let mut one_time_keys = BTreeMap::new();
        one_time_keys
            .entry(bob.user_id().clone())
            .or_insert_with(BTreeMap::new)
            .insert(bob.device_id().into(), one_time);

        let response = KeyClaimResponse::new(one_time_keys);

        manager.receive_keys_claim_response(&response).await.unwrap();

        assert!(manager
            .get_missing_sessions(&mut [bob.user_id().clone()].iter())
            .await
            .unwrap()
            .is_none());
    }

    // This test doesn't run on macos because we're modifying the session
    // creation time so we can get around the UNWEDGING_INTERVAL.
    #[async_test]
    #[cfg(target_os = "linux")]
    async fn session_unwedging() {
        use matrix_sdk_common::instant::{Duration, Instant};
        use ruma::DeviceKeyAlgorithm;

        let manager = session_manager().await;
        let bob = bob_account();
        let (_, mut session) = bob.create_session_for(&manager.account).await;

        let bob_device = ReadOnlyDevice::from_account(&bob).await;
        session.creation_time = Arc::new(Instant::now() - Duration::from_secs(3601));

        manager.store.save_devices(&[bob_device.clone()]).await.unwrap();
        manager.store.save_sessions(&[session]).await.unwrap();

        assert!(manager
            .get_missing_sessions(&mut [bob.user_id().clone()].iter())
            .await
            .unwrap()
            .is_none());

        let curve_key = bob_device.get_key(DeviceKeyAlgorithm::Curve25519).unwrap();

        assert!(!manager.users_for_key_claim.contains_key(bob.user_id()));
        assert!(!manager.is_device_wedged(&bob_device));
        manager.mark_device_as_wedged(bob_device.user_id(), curve_key).await.unwrap();
        assert!(manager.is_device_wedged(&bob_device));
        assert!(manager.users_for_key_claim.contains_key(bob.user_id()));

        let (_, request) = manager
            .get_missing_sessions(&mut [bob.user_id().clone()].iter())
            .await
            .unwrap()
            .unwrap();

        assert!(request.one_time_keys.contains_key(bob.user_id()));

        bob.generate_one_time_keys_helper(1).await;
        let one_time = bob.signed_one_time_keys_helper().await.unwrap();
        bob.mark_keys_as_published().await;

        let mut one_time_keys = BTreeMap::new();
        one_time_keys
            .entry(bob.user_id().clone())
            .or_insert_with(BTreeMap::new)
            .insert(bob.device_id().into(), one_time);

        let response = KeyClaimResponse::new(one_time_keys);

        assert!(manager.outgoing_to_device_requests.is_empty());

        manager.receive_keys_claim_response(&response).await.unwrap();

        assert!(!manager.is_device_wedged(&bob_device));
        assert!(manager
            .get_missing_sessions(&mut [bob.user_id().clone()].iter())
            .await
            .unwrap()
            .is_none());
        assert!(!manager.outgoing_to_device_requests.is_empty())
    }
}

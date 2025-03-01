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

use std::sync::Arc;

use ruma::{
    events::{
        presence::PresenceEvent,
        room::{member::MemberEventContent, power_levels::SyncPowerLevelsEvent},
    },
    MxcUri, UserId,
};

use crate::deserialized_responses::MemberEvent;

/// A member of a room.
#[derive(Clone, Debug)]
pub struct RoomMember {
    pub(crate) event: Arc<MemberEvent>,
    pub(crate) profile: Arc<Option<MemberEventContent>>,
    #[allow(dead_code)]
    pub(crate) presence: Arc<Option<PresenceEvent>>,
    pub(crate) power_levels: Arc<Option<SyncPowerLevelsEvent>>,
    pub(crate) max_power_level: i64,
    pub(crate) is_room_creator: bool,
    pub(crate) display_name_ambiguous: bool,
}

impl RoomMember {
    /// Get the unique user id of this member.
    pub fn user_id(&self) -> &UserId {
        &self.event.state_key
    }

    /// Get the display name of the member if there is one.
    pub fn display_name(&self) -> Option<&str> {
        if let Some(p) = self.profile.as_ref() {
            p.displayname.as_deref()
        } else {
            self.event.content.displayname.as_deref()
        }
    }

    /// Get the name of the member.
    ///
    /// This returns either the display name or the local part of the user id if
    /// the member didn't set a display name.
    pub fn name(&self) -> &str {
        if let Some(d) = self.display_name() {
            d
        } else {
            self.user_id().localpart()
        }
    }

    /// Get the avatar url of the member, if there is one.
    pub fn avatar_url(&self) -> Option<&MxcUri> {
        match self.profile.as_ref() {
            Some(p) => p.avatar_url.as_ref(),
            None => self.event.content.avatar_url.as_ref(),
        }
    }

    /// Get the normalized power level of this member.
    ///
    /// The normalized power level depends on the maximum power level that can
    /// be found in a certain room, it's always in the range of 0-100.
    pub fn normalized_power_level(&self) -> i64 {
        if self.max_power_level > 0 {
            (self.power_level() * 100) / self.max_power_level
        } else {
            self.power_level()
        }
    }

    /// Get the power level of this member.
    pub fn power_level(&self) -> i64 {
        self.power_levels
            .as_ref()
            .as_ref()
            .map(|e| {
                e.content
                    .users
                    .get(self.user_id())
                    .map(|p| (*p).into())
                    .unwrap_or_else(|| e.content.users_default.into())
            })
            .unwrap_or_else(|| if self.is_room_creator { 100 } else { 0 })
    }

    /// Is the name that the member uses ambiguous in the room.
    ///
    /// A name is considered to be ambiguous if at least one other member shares
    /// the same name.
    pub fn name_ambiguous(&self) -> bool {
        self.display_name_ambiguous
    }
}

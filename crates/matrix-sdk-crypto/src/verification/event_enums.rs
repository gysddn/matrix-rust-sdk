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

use std::{
    collections::BTreeMap,
    convert::{TryFrom, TryInto},
};

use ruma::{
    events::{
        key::verification::{
            accept::{AcceptEventContent, AcceptMethod, ToDeviceAcceptEventContent},
            cancel::{CancelCode, CancelEventContent, ToDeviceCancelEventContent},
            done::{DoneEventContent, ToDeviceDoneEventContent},
            key::{KeyEventContent, ToDeviceKeyEventContent},
            mac::{MacEventContent, ToDeviceMacEventContent},
            ready::{ReadyEventContent, ToDeviceReadyEventContent},
            request::ToDeviceRequestEventContent,
            start::{StartEventContent, StartMethod, ToDeviceStartEventContent},
            VerificationMethod,
        },
        room::message::{KeyVerificationRequestEventContent, MessageType},
        AnyMessageEvent, AnyMessageEventContent, AnyToDeviceEvent, AnyToDeviceEventContent,
    },
    serde::CanonicalJsonValue,
    DeviceId, MilliSecondsSinceUnixEpoch, RoomId, UserId,
};

use super::FlowId;

#[derive(Debug)]
pub enum AnyEvent<'a> {
    Room(&'a AnyMessageEvent),
    ToDevice(&'a AnyToDeviceEvent),
}

impl AnyEvent<'_> {
    pub fn sender(&self) -> &UserId {
        match self {
            Self::Room(e) => e.sender(),
            Self::ToDevice(e) => e.sender(),
        }
    }

    pub fn timestamp(&self) -> Option<&MilliSecondsSinceUnixEpoch> {
        match self {
            AnyEvent::Room(e) => Some(e.origin_server_ts()),
            AnyEvent::ToDevice(e) => match e {
                AnyToDeviceEvent::KeyVerificationRequest(e) => Some(&e.content.timestamp),
                _ => None,
            },
        }
    }

    pub fn is_room_event(&self) -> bool {
        matches!(self, AnyEvent::Room(_))
    }

    pub fn verification_content(&self) -> Option<AnyVerificationContent> {
        match self {
            AnyEvent::Room(e) => match e {
                AnyMessageEvent::CallAnswer(_)
                | AnyMessageEvent::CallInvite(_)
                | AnyMessageEvent::CallHangup(_)
                | AnyMessageEvent::CallCandidates(_)
                | AnyMessageEvent::Reaction(_)
                | AnyMessageEvent::RoomEncrypted(_)
                | AnyMessageEvent::RoomMessageFeedback(_)
                | AnyMessageEvent::RoomRedaction(_)
                | AnyMessageEvent::Sticker(_) => None,
                AnyMessageEvent::RoomMessage(m) => {
                    if let MessageType::VerificationRequest(v) = &m.content.msgtype {
                        Some(RequestContent::from(v).into())
                    } else {
                        None
                    }
                }
                AnyMessageEvent::KeyVerificationReady(e) => {
                    Some(ReadyContent::from(&e.content).into())
                }
                AnyMessageEvent::KeyVerificationStart(e) => {
                    Some(StartContent::from(&e.content).into())
                }
                AnyMessageEvent::KeyVerificationCancel(e) => {
                    Some(CancelContent::from(&e.content).into())
                }
                AnyMessageEvent::KeyVerificationAccept(e) => {
                    Some(AcceptContent::from(&e.content).into())
                }
                AnyMessageEvent::KeyVerificationKey(e) => Some(KeyContent::from(&e.content).into()),
                AnyMessageEvent::KeyVerificationMac(e) => Some(MacContent::from(&e.content).into()),
                AnyMessageEvent::KeyVerificationDone(e) => {
                    Some(DoneContent::from(&e.content).into())
                }
                _ => None,
            },
            AnyEvent::ToDevice(e) => match e {
                AnyToDeviceEvent::Dummy(_)
                | AnyToDeviceEvent::RoomKey(_)
                | AnyToDeviceEvent::RoomKeyRequest(_)
                | AnyToDeviceEvent::ForwardedRoomKey(_)
                | AnyToDeviceEvent::RoomEncrypted(_) => None,
                AnyToDeviceEvent::KeyVerificationRequest(e) => {
                    Some(RequestContent::from(&e.content).into())
                }
                AnyToDeviceEvent::KeyVerificationReady(e) => {
                    Some(ReadyContent::from(&e.content).into())
                }
                AnyToDeviceEvent::KeyVerificationStart(e) => {
                    Some(StartContent::from(&e.content).into())
                }
                AnyToDeviceEvent::KeyVerificationCancel(e) => {
                    Some(CancelContent::from(&e.content).into())
                }
                AnyToDeviceEvent::KeyVerificationAccept(e) => {
                    Some(AcceptContent::from(&e.content).into())
                }
                AnyToDeviceEvent::KeyVerificationKey(e) => {
                    Some(KeyContent::from(&e.content).into())
                }
                AnyToDeviceEvent::KeyVerificationMac(e) => {
                    Some(MacContent::from(&e.content).into())
                }
                AnyToDeviceEvent::KeyVerificationDone(e) => {
                    Some(DoneContent::from(&e.content).into())
                }
                _ => None,
            },
        }
    }
}

impl<'a> From<&'a AnyMessageEvent> for AnyEvent<'a> {
    fn from(e: &'a AnyMessageEvent) -> Self {
        Self::Room(e)
    }
}

impl<'a> From<&'a AnyToDeviceEvent> for AnyEvent<'a> {
    fn from(e: &'a AnyToDeviceEvent) -> Self {
        Self::ToDevice(e)
    }
}

impl TryFrom<&AnyEvent<'_>> for FlowId {
    type Error = ();

    fn try_from(value: &AnyEvent<'_>) -> Result<Self, Self::Error> {
        match value {
            AnyEvent::Room(e) => FlowId::try_from(*e),
            AnyEvent::ToDevice(e) => FlowId::try_from(*e),
        }
    }
}

impl TryFrom<&AnyMessageEvent> for FlowId {
    type Error = ();

    fn try_from(value: &AnyMessageEvent) -> Result<Self, Self::Error> {
        match value {
            AnyMessageEvent::CallAnswer(_)
            | AnyMessageEvent::CallInvite(_)
            | AnyMessageEvent::CallHangup(_)
            | AnyMessageEvent::CallCandidates(_)
            | AnyMessageEvent::Reaction(_)
            | AnyMessageEvent::RoomEncrypted(_)
            | AnyMessageEvent::RoomMessageFeedback(_)
            | AnyMessageEvent::RoomRedaction(_)
            | AnyMessageEvent::Sticker(_) => Err(()),
            AnyMessageEvent::KeyVerificationReady(e) => {
                Ok(FlowId::from((&e.room_id, &e.content.relates_to.event_id)))
            }
            AnyMessageEvent::RoomMessage(e) => Ok(FlowId::from((&e.room_id, &e.event_id))),
            AnyMessageEvent::KeyVerificationStart(e) => {
                Ok(FlowId::from((&e.room_id, &e.content.relates_to.event_id)))
            }
            AnyMessageEvent::KeyVerificationCancel(e) => {
                Ok(FlowId::from((&e.room_id, &e.content.relates_to.event_id)))
            }
            AnyMessageEvent::KeyVerificationAccept(e) => {
                Ok(FlowId::from((&e.room_id, &e.content.relates_to.event_id)))
            }
            AnyMessageEvent::KeyVerificationKey(e) => {
                Ok(FlowId::from((&e.room_id, &e.content.relates_to.event_id)))
            }
            AnyMessageEvent::KeyVerificationMac(e) => {
                Ok(FlowId::from((&e.room_id, &e.content.relates_to.event_id)))
            }
            AnyMessageEvent::KeyVerificationDone(e) => {
                Ok(FlowId::from((&e.room_id, &e.content.relates_to.event_id)))
            }
            _ => Err(()),
        }
    }
}

impl TryFrom<&AnyToDeviceEvent> for FlowId {
    type Error = ();

    fn try_from(value: &AnyToDeviceEvent) -> Result<Self, Self::Error> {
        match value {
            AnyToDeviceEvent::Dummy(_)
            | AnyToDeviceEvent::RoomKey(_)
            | AnyToDeviceEvent::RoomKeyRequest(_)
            | AnyToDeviceEvent::ForwardedRoomKey(_)
            | AnyToDeviceEvent::RoomEncrypted(_) => Err(()),
            AnyToDeviceEvent::KeyVerificationRequest(e) => {
                Ok(FlowId::from(e.content.transaction_id.to_owned()))
            }
            AnyToDeviceEvent::KeyVerificationReady(e) => {
                Ok(FlowId::from(e.content.transaction_id.to_owned()))
            }
            AnyToDeviceEvent::KeyVerificationStart(e) => {
                Ok(FlowId::from(e.content.transaction_id.to_owned()))
            }
            AnyToDeviceEvent::KeyVerificationCancel(e) => {
                Ok(FlowId::from(e.content.transaction_id.to_owned()))
            }
            AnyToDeviceEvent::KeyVerificationAccept(e) => {
                Ok(FlowId::from(e.content.transaction_id.to_owned()))
            }
            AnyToDeviceEvent::KeyVerificationKey(e) => {
                Ok(FlowId::from(e.content.transaction_id.to_owned()))
            }
            AnyToDeviceEvent::KeyVerificationMac(e) => {
                Ok(FlowId::from(e.content.transaction_id.to_owned()))
            }
            AnyToDeviceEvent::KeyVerificationDone(e) => {
                Ok(FlowId::from(e.content.transaction_id.to_owned()))
            }
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
pub enum AnyVerificationContent<'a> {
    Request(RequestContent<'a>),
    Ready(ReadyContent<'a>),
    Cancel(CancelContent<'a>),
    Start(StartContent<'a>),
    Done(DoneContent<'a>),
    Accept(AcceptContent<'a>),
    Key(KeyContent<'a>),
    Mac(MacContent<'a>),
}

#[derive(Debug)]
pub enum RequestContent<'a> {
    ToDevice(&'a ToDeviceRequestEventContent),
    Room(&'a KeyVerificationRequestEventContent),
}

impl RequestContent<'_> {
    pub fn from_device(&self) -> &DeviceId {
        match self {
            Self::ToDevice(t) => &t.from_device,
            Self::Room(r) => &r.from_device,
        }
    }

    pub fn methods(&self) -> &[VerificationMethod] {
        match self {
            Self::ToDevice(t) => &t.methods,
            Self::Room(r) => &r.methods,
        }
    }
}

#[derive(Debug)]
pub enum ReadyContent<'a> {
    ToDevice(&'a ToDeviceReadyEventContent),
    Room(&'a ReadyEventContent),
}

impl ReadyContent<'_> {
    pub fn from_device(&self) -> &DeviceId {
        match self {
            Self::ToDevice(t) => &t.from_device,
            Self::Room(r) => &r.from_device,
        }
    }

    pub fn methods(&self) -> &[VerificationMethod] {
        match self {
            Self::ToDevice(t) => &t.methods,
            Self::Room(r) => &r.methods,
        }
    }
}

macro_rules! from_for_enum {
    ($type: ident, $enum_variant: ident, $enum: ident) => {
        impl<'a> From<$type<'a>> for $enum<'a> {
            fn from(c: $type<'a>) -> Self {
                Self::$enum_variant(c)
            }
        }
    };
}

macro_rules! from_borrow_for_enum {
    ($type: ident, $enum_variant: ident, $enum: ident) => {
        impl<'a> From<&'a $type> for $enum<'a> {
            fn from(c: &'a $type) -> Self {
                Self::$enum_variant(c)
            }
        }
    };
}

from_for_enum!(RequestContent, Request, AnyVerificationContent);
from_for_enum!(ReadyContent, Ready, AnyVerificationContent);
from_for_enum!(CancelContent, Cancel, AnyVerificationContent);
from_for_enum!(StartContent, Start, AnyVerificationContent);
from_for_enum!(DoneContent, Done, AnyVerificationContent);
from_for_enum!(AcceptContent, Accept, AnyVerificationContent);
from_for_enum!(KeyContent, Key, AnyVerificationContent);
from_for_enum!(MacContent, Mac, AnyVerificationContent);

from_borrow_for_enum!(ToDeviceRequestEventContent, ToDevice, RequestContent);
from_borrow_for_enum!(KeyVerificationRequestEventContent, Room, RequestContent);
from_borrow_for_enum!(ReadyEventContent, Room, ReadyContent);
from_borrow_for_enum!(ToDeviceReadyEventContent, ToDevice, ReadyContent);
from_borrow_for_enum!(StartEventContent, Room, StartContent);
from_borrow_for_enum!(ToDeviceStartEventContent, ToDevice, StartContent);
from_borrow_for_enum!(AcceptEventContent, Room, AcceptContent);
from_borrow_for_enum!(ToDeviceAcceptEventContent, ToDevice, AcceptContent);
from_borrow_for_enum!(KeyEventContent, Room, KeyContent);
from_borrow_for_enum!(ToDeviceKeyEventContent, ToDevice, KeyContent);
from_borrow_for_enum!(MacEventContent, Room, MacContent);
from_borrow_for_enum!(ToDeviceMacEventContent, ToDevice, MacContent);
from_borrow_for_enum!(CancelEventContent, Room, CancelContent);
from_borrow_for_enum!(ToDeviceCancelEventContent, ToDevice, CancelContent);

macro_rules! try_from_outgoing_content {
    ($type: ident, $enum_variant: ident) => {
        impl<'a> TryFrom<&'a OutgoingContent> for $type<'a> {
            type Error = ();

            fn try_from(value: &'a OutgoingContent) -> Result<Self, Self::Error> {
                match value {
                    OutgoingContent::Room(_, c) => {
                        if let AnyMessageEventContent::$enum_variant(c) = c {
                            Ok(Self::Room(c))
                        } else {
                            Err(())
                        }
                    }
                    OutgoingContent::ToDevice(c) => {
                        if let AnyToDeviceEventContent::$enum_variant(c) = c {
                            Ok(Self::ToDevice(c))
                        } else {
                            Err(())
                        }
                    }
                }
            }
        }
    };
}

try_from_outgoing_content!(ReadyContent, KeyVerificationReady);
try_from_outgoing_content!(StartContent, KeyVerificationStart);
try_from_outgoing_content!(AcceptContent, KeyVerificationAccept);
try_from_outgoing_content!(KeyContent, KeyVerificationKey);
try_from_outgoing_content!(MacContent, KeyVerificationMac);
try_from_outgoing_content!(CancelContent, KeyVerificationCancel);
try_from_outgoing_content!(DoneContent, KeyVerificationDone);

impl<'a> TryFrom<&'a OutgoingContent> for RequestContent<'a> {
    type Error = ();

    fn try_from(value: &'a OutgoingContent) -> Result<Self, Self::Error> {
        match value {
            OutgoingContent::Room(_, c) => {
                if let AnyMessageEventContent::RoomMessage(m) = c {
                    if let MessageType::VerificationRequest(c) = &m.msgtype {
                        Ok(Self::Room(c))
                    } else {
                        Err(())
                    }
                } else {
                    Err(())
                }
            }
            OutgoingContent::ToDevice(c) => {
                if let AnyToDeviceEventContent::KeyVerificationRequest(c) = c {
                    Ok(Self::ToDevice(c))
                } else {
                    Err(())
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum StartContent<'a> {
    ToDevice(&'a ToDeviceStartEventContent),
    Room(&'a StartEventContent),
}

impl<'a> StartContent<'a> {
    pub fn from_device(&self) -> &DeviceId {
        match self {
            Self::ToDevice(c) => &c.from_device,
            Self::Room(c) => &c.from_device,
        }
    }

    pub fn flow_id(&self) -> &str {
        match self {
            Self::ToDevice(c) => &c.transaction_id,
            Self::Room(c) => c.relates_to.event_id.as_str(),
        }
    }

    pub fn method(&self) -> &StartMethod {
        match self {
            Self::ToDevice(c) => &c.method,
            Self::Room(c) => &c.method,
        }
    }

    pub fn canonical_json(&self) -> CanonicalJsonValue {
        let content = match self {
            Self::ToDevice(c) => serde_json::to_value(c),
            Self::Room(c) => serde_json::to_value(c),
        };

        content.expect("Can't serialize content").try_into().expect("Can't canonicalize content")
    }
}

impl<'a> From<&'a OwnedStartContent> for StartContent<'a> {
    fn from(c: &'a OwnedStartContent) -> Self {
        match c {
            OwnedStartContent::ToDevice(c) => Self::ToDevice(c),
            OwnedStartContent::Room(_, c) => Self::Room(c),
        }
    }
}

#[derive(Debug)]
pub enum DoneContent<'a> {
    ToDevice(&'a ToDeviceDoneEventContent),
    Room(&'a DoneEventContent),
}

impl<'a> From<&'a DoneEventContent> for DoneContent<'a> {
    fn from(c: &'a DoneEventContent) -> Self {
        Self::Room(c)
    }
}

impl<'a> From<&'a ToDeviceDoneEventContent> for DoneContent<'a> {
    fn from(c: &'a ToDeviceDoneEventContent) -> Self {
        Self::ToDevice(c)
    }
}

impl<'a> DoneContent<'a> {
    pub fn flow_id(&self) -> &str {
        match self {
            Self::ToDevice(c) => &c.transaction_id,
            Self::Room(c) => c.relates_to.event_id.as_str(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum AcceptContent<'a> {
    ToDevice(&'a ToDeviceAcceptEventContent),
    Room(&'a AcceptEventContent),
}

impl AcceptContent<'_> {
    pub fn flow_id(&self) -> &str {
        match self {
            Self::ToDevice(c) => &c.transaction_id,
            Self::Room(c) => c.relates_to.event_id.as_str(),
        }
    }

    pub fn method(&self) -> &AcceptMethod {
        match self {
            Self::ToDevice(c) => &c.method,
            Self::Room(c) => &c.method,
        }
    }
}

impl<'a> From<&'a OwnedAcceptContent> for AcceptContent<'a> {
    fn from(content: &'a OwnedAcceptContent) -> Self {
        match content {
            OwnedAcceptContent::ToDevice(c) => Self::ToDevice(c),
            OwnedAcceptContent::Room(_, c) => Self::Room(c),
        }
    }
}

#[derive(Clone, Debug)]
pub enum KeyContent<'a> {
    ToDevice(&'a ToDeviceKeyEventContent),
    Room(&'a KeyEventContent),
}

impl KeyContent<'_> {
    pub fn flow_id(&self) -> &str {
        match self {
            Self::ToDevice(c) => &c.transaction_id,
            Self::Room(c) => c.relates_to.event_id.as_str(),
        }
    }

    pub fn public_key(&self) -> &str {
        match self {
            Self::ToDevice(c) => &c.key,
            Self::Room(c) => &c.key,
        }
    }
}

#[derive(Clone, Debug)]
pub enum MacContent<'a> {
    ToDevice(&'a ToDeviceMacEventContent),
    Room(&'a MacEventContent),
}

impl MacContent<'_> {
    pub fn flow_id(&self) -> &str {
        match self {
            Self::ToDevice(c) => &c.transaction_id,
            Self::Room(c) => c.relates_to.event_id.as_str(),
        }
    }

    pub fn mac(&self) -> &BTreeMap<String, String> {
        match self {
            Self::ToDevice(c) => &c.mac,
            Self::Room(c) => &c.mac,
        }
    }

    pub fn keys(&self) -> &str {
        match self {
            Self::ToDevice(c) => &c.keys,
            Self::Room(c) => &c.keys,
        }
    }
}

#[derive(Clone, Debug)]
pub enum CancelContent<'a> {
    ToDevice(&'a ToDeviceCancelEventContent),
    Room(&'a CancelEventContent),
}

impl CancelContent<'_> {
    pub fn cancel_code(&self) -> &CancelCode {
        match self {
            Self::ToDevice(c) => &c.code,
            Self::Room(c) => &c.code,
        }
    }
}

#[derive(Clone, Debug)]
pub enum OwnedStartContent {
    ToDevice(ToDeviceStartEventContent),
    Room(RoomId, StartEventContent),
}

impl OwnedStartContent {
    pub fn method(&self) -> &StartMethod {
        match self {
            Self::ToDevice(c) => &c.method,
            Self::Room(_, c) => &c.method,
        }
    }

    pub fn method_mut(&mut self) -> &mut StartMethod {
        match self {
            Self::ToDevice(c) => &mut c.method,
            Self::Room(_, c) => &mut c.method,
        }
    }

    pub fn as_start_content(&self) -> StartContent<'_> {
        match self {
            Self::ToDevice(c) => StartContent::ToDevice(c),
            Self::Room(_, c) => StartContent::Room(c),
        }
    }

    pub fn flow_id(&self) -> FlowId {
        match self {
            Self::ToDevice(c) => FlowId::ToDevice(c.transaction_id.clone()),
            Self::Room(r, c) => FlowId::InRoom(r.clone(), c.relates_to.event_id.clone()),
        }
    }

    pub fn canonical_json(self) -> CanonicalJsonValue {
        let content = match self {
            Self::ToDevice(c) => serde_json::to_value(c),
            Self::Room(_, c) => serde_json::to_value(c),
        };

        content.expect("Can't serialize content").try_into().expect("Can't canonicalize content")
    }
}

impl From<(RoomId, StartEventContent)> for OwnedStartContent {
    fn from(tuple: (RoomId, StartEventContent)) -> Self {
        Self::Room(tuple.0, tuple.1)
    }
}

impl From<ToDeviceStartEventContent> for OwnedStartContent {
    fn from(content: ToDeviceStartEventContent) -> Self {
        Self::ToDevice(content)
    }
}

#[derive(Clone, Debug)]
pub enum OwnedAcceptContent {
    ToDevice(ToDeviceAcceptEventContent),
    Room(RoomId, AcceptEventContent),
}

impl From<ToDeviceAcceptEventContent> for OwnedAcceptContent {
    fn from(content: ToDeviceAcceptEventContent) -> Self {
        Self::ToDevice(content)
    }
}

impl From<(RoomId, AcceptEventContent)> for OwnedAcceptContent {
    fn from(content: (RoomId, AcceptEventContent)) -> Self {
        Self::Room(content.0, content.1)
    }
}

impl OwnedAcceptContent {
    pub fn method_mut(&mut self) -> &mut AcceptMethod {
        match self {
            Self::ToDevice(c) => &mut c.method,
            Self::Room(_, c) => &mut c.method,
        }
    }
}

#[derive(Clone, Debug)]
pub enum OutgoingContent {
    Room(RoomId, AnyMessageEventContent),
    ToDevice(AnyToDeviceEventContent),
}

impl From<OwnedStartContent> for OutgoingContent {
    fn from(content: OwnedStartContent) -> Self {
        match content {
            OwnedStartContent::Room(r, c) => {
                (r, AnyMessageEventContent::KeyVerificationStart(c)).into()
            }
            OwnedStartContent::ToDevice(c) => {
                AnyToDeviceEventContent::KeyVerificationStart(c).into()
            }
        }
    }
}

impl From<AnyToDeviceEventContent> for OutgoingContent {
    fn from(content: AnyToDeviceEventContent) -> Self {
        OutgoingContent::ToDevice(content)
    }
}

impl From<(RoomId, AnyMessageEventContent)> for OutgoingContent {
    fn from(content: (RoomId, AnyMessageEventContent)) -> Self {
        OutgoingContent::Room(content.0, content.1)
    }
}

use crate::{OutgoingRequest, OutgoingVerificationRequest, RoomMessageRequest, ToDeviceRequest};

impl TryFrom<OutgoingVerificationRequest> for OutgoingContent {
    type Error = String;

    fn try_from(request: OutgoingVerificationRequest) -> Result<Self, Self::Error> {
        match request {
            OutgoingVerificationRequest::ToDevice(r) => Self::try_from(r),
            OutgoingVerificationRequest::InRoom(r) => Ok(Self::from(r)),
        }
    }
}

impl From<RoomMessageRequest> for OutgoingContent {
    fn from(value: RoomMessageRequest) -> Self {
        (value.room_id, value.content).into()
    }
}

impl TryFrom<ToDeviceRequest> for OutgoingContent {
    type Error = String;

    fn try_from(request: ToDeviceRequest) -> Result<Self, Self::Error> {
        use ruma::events::EventType;
        use serde_json::Value;

        let json: Value = serde_json::from_str(
            request
                .messages
                .values()
                .next()
                .and_then(|m| m.values().next())
                .map(|c| c.json().get())
                .ok_or_else(|| "Content is missing from the request".to_owned())?,
        )
        .map_err(|e| e.to_string())?;

        let content = match request.event_type {
            EventType::KeyVerificationStart => AnyToDeviceEventContent::KeyVerificationStart(
                serde_json::from_value(json).map_err(|e| e.to_string())?,
            ),
            EventType::KeyVerificationKey => AnyToDeviceEventContent::KeyVerificationKey(
                serde_json::from_value(json).map_err(|e| e.to_string())?,
            ),
            EventType::KeyVerificationAccept => AnyToDeviceEventContent::KeyVerificationAccept(
                serde_json::from_value(json).map_err(|e| e.to_string())?,
            ),
            EventType::KeyVerificationMac => AnyToDeviceEventContent::KeyVerificationMac(
                serde_json::from_value(json).map_err(|e| e.to_string())?,
            ),
            EventType::KeyVerificationCancel => AnyToDeviceEventContent::KeyVerificationCancel(
                serde_json::from_value(json).map_err(|e| e.to_string())?,
            ),
            EventType::KeyVerificationReady => AnyToDeviceEventContent::KeyVerificationReady(
                serde_json::from_value(json).map_err(|e| e.to_string())?,
            ),
            EventType::KeyVerificationDone => AnyToDeviceEventContent::KeyVerificationDone(
                serde_json::from_value(json).map_err(|e| e.to_string())?,
            ),
            EventType::KeyVerificationRequest => AnyToDeviceEventContent::KeyVerificationRequest(
                serde_json::from_value(json).map_err(|e| e.to_string())?,
            ),
            e => return Err(format!("Unsupported event type {}", e)),
        };

        Ok(content.into())
    }
}

impl TryFrom<OutgoingRequest> for OutgoingContent {
    type Error = String;

    fn try_from(value: OutgoingRequest) -> Result<Self, Self::Error> {
        match value.request() {
            crate::OutgoingRequests::KeysUpload(_) => Err("Invalid request type".to_owned()),
            crate::OutgoingRequests::KeysQuery(_) => Err("Invalid request type".to_owned()),
            crate::OutgoingRequests::ToDeviceRequest(r) => Self::try_from(r.clone()),
            crate::OutgoingRequests::SignatureUpload(_) => Err("Invalid request type".to_owned()),
            crate::OutgoingRequests::KeysClaim(_) => Err("Invalid request type".to_owned()),
            crate::OutgoingRequests::RoomMessage(r) => Ok(Self::from(r.clone())),
        }
    }
}

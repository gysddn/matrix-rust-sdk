A no-network-IO implementation of a state machine that handles E2EE for
[Matrix] clients.

# Usage

If you're just trying to write a Matrix client or bot in Rust, you're probably
looking for [matrix-sdk] instead.

However, if you're looking to add E2EE to an existing Matrix client or library,
read on.

The state machine works in a push/pull manner:

- you push state changes and events retrieved from a Matrix homeserver /sync
  response into the state machine
- you pull requests that you'll need to send back to the homeserver out of the
  state machine

```rust,no_run
use std::{collections::BTreeMap, convert::TryFrom};

use matrix_sdk_crypto::{OlmMachine, OlmError};
use ruma::{UserId, api::client::r0::sync::sync_events::{ToDevice, DeviceLists}};

#[tokio::main]
async fn main() -> Result<(), OlmError> {
    let alice = UserId::try_from("@alice:example.org").unwrap();
    let machine = OlmMachine::new(&alice, "DEVICEID".into());

    let to_device_events = ToDevice::default();
    let changed_devices = DeviceLists::default();
    let one_time_key_counts = BTreeMap::default();

    // Push changes that the server sent to us in a sync response.
    let decrypted_to_device = machine.receive_sync_changes(
        to_device_events,
        &changed_devices,
        &one_time_key_counts
    ).await?;

    // Pull requests that we need to send out.
    let outgoing_requests = machine.outgoing_requests().await?;

    // Send the requests here out and call machine.mark_request_as_sent().

    Ok(())
}
```

[Matrix]: https://matrix.org/
[matrix-sdk]: https://github.com/matrix-org/matrix-rust-sdk/

# Room key sharing algorithm

The decision tree below visualizes the way this crate decides whether a room
key will be shared with a requester upon a key request.

![](https://raw.githubusercontent.com/matrix-org/matrix-rust-sdk/main/contrib/key-sharing-algorithm/model.png)

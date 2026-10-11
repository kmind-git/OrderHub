// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Message senders for standalone clients and live runtime dispatch.

use std::thread::{self, ThreadId};

use super::dispatch::DispatchMessage;

/// A message sender which preserves callback ancestry when bound to a live runtime.
///
/// Sends on the runtime owner thread capture its active root. Sends on other threads
/// and sends without an active root are independent ingress. Conversion from a plain Tokio sender supports standalone
/// clients whose receivers consume domain events directly, without callback tracking.
#[derive(Debug)]
pub struct DispatchSender<T> {
    channel: DispatchChannel<T>,
}

/// A sender for data, execution, and system events.
pub type EventSender<T> = DispatchSender<T>;

impl<T> DispatchSender<T> {
    /// Binds a dispatch channel to the calling runtime thread.
    #[must_use]
    pub fn new(sender: tokio::sync::mpsc::UnboundedSender<DispatchMessage<T>>) -> Self {
        Self {
            channel: DispatchChannel::Dispatch {
                sender,
                owner: thread::current().id(),
            },
        }
    }

    // panics-doc-ok
    /// Sends a message, preserving the active owner-thread callback root.
    ///
    /// # Errors
    ///
    /// Returns the undelivered envelope if the receiver is closed or a bounded queue is full.
    ///
    /// # Panics
    ///
    /// Panics if the owner exhausts its channel context IDs.
    pub fn send(
        &self,
        message: T,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<DispatchMessage<T>>> {
        match &self.channel {
            DispatchChannel::Plain(sender) => sender
                .send(message)
                .map_err(|e| tokio::sync::mpsc::error::SendError(e.0.into())),
            DispatchChannel::Bounded(sender) => sender
                .try_send(message)
                .map_err(|e| tokio::sync::mpsc::error::SendError(e.into_inner().into())),
            DispatchChannel::Dispatch { sender, owner } => {
                sender.send(DispatchMessage::new(message, *owner))
            }
        }
    }

    /// Returns whether the receiver has closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        match &self.channel {
            DispatchChannel::Plain(sender) => sender.is_closed(),
            DispatchChannel::Bounded(sender) => sender.is_closed(),
            DispatchChannel::Dispatch { sender, .. } => sender.is_closed(),
        }
    }

    /// Returns whether both senders target the same channel.
    #[must_use]
    pub fn same_channel(&self, other: &Self) -> bool {
        match (&self.channel, &other.channel) {
            (DispatchChannel::Plain(left), DispatchChannel::Plain(right)) => {
                left.same_channel(right)
            }
            (DispatchChannel::Bounded(left), DispatchChannel::Bounded(right)) => {
                left.same_channel(right)
            }
            (
                DispatchChannel::Dispatch { sender: left, .. },
                DispatchChannel::Dispatch { sender: right, .. },
            ) => left.same_channel(right),
            _ => false,
        }
    }
}

impl<T> Clone for DispatchSender<T> {
    fn clone(&self) -> Self {
        let channel = match &self.channel {
            DispatchChannel::Plain(sender) => DispatchChannel::Plain(sender.clone()),
            DispatchChannel::Bounded(sender) => DispatchChannel::Bounded(sender.clone()),
            DispatchChannel::Dispatch { sender, owner } => DispatchChannel::Dispatch {
                sender: sender.clone(),
                owner: *owner,
            },
        };

        Self { channel }
    }
}

impl<T> From<tokio::sync::mpsc::UnboundedSender<T>> for DispatchSender<T> {
    fn from(sender: tokio::sync::mpsc::UnboundedSender<T>) -> Self {
        Self {
            channel: DispatchChannel::Plain(sender),
        }
    }
}

impl<T> From<tokio::sync::mpsc::Sender<T>> for DispatchSender<T> {
    fn from(sender: tokio::sync::mpsc::Sender<T>) -> Self {
        Self {
            channel: DispatchChannel::Bounded(sender),
        }
    }
}

#[derive(Debug)]
enum DispatchChannel<T> {
    Plain(tokio::sync::mpsc::UnboundedSender<T>),
    Bounded(tokio::sync::mpsc::Sender<T>),
    Dispatch {
        sender: tokio::sync::mpsc::UnboundedSender<DispatchMessage<T>>,
        owner: ThreadId,
    },
}

#[cfg(test)]
mod tests {
    use super::DispatchSender;

    #[test]
    fn test_bounded_sender_returns_payload_on_full_and_closed_without_blocking() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let sender = DispatchSender::from(tx);
        assert!(sender.same_channel(&sender.clone()));
        sender.send(1).unwrap();
        let returned = sender.send(2).unwrap_err().0.dispatch(|value| value);
        assert_eq!(returned, 2);
        assert_eq!(rx.try_recv().unwrap(), 1);
        sender.send(3).unwrap();
        drop(rx);
        assert!(sender.is_closed());
        assert_eq!(sender.send(4).unwrap_err().0.dispatch(|value| value), 4);
        let (other, _) = tokio::sync::mpsc::channel(1);
        assert!(!sender.same_channel(&DispatchSender::from(other)));
    }
}

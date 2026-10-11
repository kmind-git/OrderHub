# Production PostgreSQL path no-C link probe

This standalone crate depends on the retained `nautilus-infrastructure` library with only
its `postgres` feature. It links a runnable executable and exercises connection option
formatting without opening a socket. The workspace's own infrastructure tests select
unrelated development dependencies including `zstd-sys`, so they cannot isolate the
normal/build PostgreSQL path. This probe does not validate authentication, transactions,
pooling, reconnection, or PostgreSQL TLS; those remain activation-time checks under the
OrderHub simplification plan. The separate SQLx TLS prototype exercises the local
custom-provider patch.

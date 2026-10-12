# OrderHub infrastructure validation

Redis and PostgreSQL implementations are retained as optional integrations. The default
OrderHub node uses its local redb business ledger and rejects database activation flags.
Service execution requires a separate integration activation gate.

## Checks without services

Compile the retained Redis restoration tests without connecting to a server:

```bash
cargo test --locked --profile nextest -p nautilus-infrastructure --features redis --test redis_restoration --no-run
```

Compile the PostgreSQL library tests separately:

```bash
cargo test --locked --profile nextest -p nautilus-infrastructure --no-default-features --features postgres --lib --no-run
```

The [schema tool](../../docs/orderhub/tooling.md) has an offline `schema-check` command.
Formatting, compilation, local certificate tests, and feature checks do not validate service
authentication, transactions, restoration, or reconnect behavior.

## Service activation

Use dedicated test instances. The tests modify data and must never point at production services.
See [Docker services](../../.docker/README.md) for optional local service setup and schema
initialization. Existing test database names are compatibility defaults.

After selecting and initializing a Redis test instance, explicitly run the ignored restoration
tests:

```bash
cargo test --locked --profile nextest -p nautilus-infrastructure --features redis --test redis_restoration -- --ignored --test-threads=1
```

Review the configuration, platform guards, and ignored tests in the affected source before
running PostgreSQL service tests. Record the tested revision, authentication mode, TLS settings,
transaction behavior, and reconnect results. Successful compilation does not activate these
integrations in the OrderHub node.

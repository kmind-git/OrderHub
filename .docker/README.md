# Docker services

`docker-compose.yml` retains PostgreSQL and Redis services for explicit integration activation.
This checkout publishes no container images. The former wheel, JupyterLab, and Python/PyO3
local development image entries are retired. Set up Rust on the host using the pinned stable
toolchain and the [environment guide](../docs/developer_guide/environment_setup.md).

The commands below are service activation instructions, not evidence that authentication,
transactions, or reconnect behavior have been validated. The default OrderHub node does not
require these services. Use dedicated test instances because database tests modify their data.

## Start test services

From the repository root:

```bash
docker compose -f .docker/docker-compose.yml up -d postgres redis
```

The compose defaults bind only to localhost: PostgreSQL port `5432`, user `nautilus`, password
`pass`, database `nautilus`; Redis port `6379`. Override the compose variables when selecting
other credentials. Wait for PostgreSQL to accept connections before initializing its schema.

## Check or initialize the schema

The offline check needs no running service:

```bash
cargo run --locked --profile nextest -p orderhub-tools --bin orderhub-db -- \
  schema-check --schema schema/sql
```

Initialize only the selected test database; the password is supplied through the environment:

```bash
POSTGRES_PASSWORD=pass cargo run --locked --profile nextest \
  -p orderhub-tools --bin orderhub-db -- \
  init --schema schema/sql --host localhost --port 5432 \
  --username nautilus --database nautilus
```

This uses the retained `schema/sql/{types,tables,functions,partitions}.sql` files.
The dedicated tool requires an explicit username and database, as described in
[OrderHub tooling](../docs/orderhub/tooling.md).

## Service tests

The retained PostgreSQL Cache integration tests execute on Linux with a dedicated instance:

```bash
POSTGRES_HOST=localhost POSTGRES_PORT=5432 POSTGRES_USERNAME=nautilus \
  POSTGRES_PASSWORD=pass POSTGRES_DATABASE=nautilus \
  cargo test --locked --profile nextest -p nautilus-infrastructure \
  --features postgres --test integration -- --test-threads=1
```

Redis bus and Cache restoration coverage is retained in `redis_restoration` and is ignored
until the separate service activation gate is selected. Configuration and validation boundaries
are described in [OrderHub tooling](../docs/orderhub/tooling.md#rust-仓库检查) and the
[contribution guide](../CONTRIBUTING.md).

The retired `init-services`, `init-db`, and Python test Make targets are not supported entries.

## Stop test services

```bash
docker compose -f .docker/docker-compose.yml stop postgres redis
```

Stopping services preserves the named PostgreSQL volume. Volume removal is a separate,
explicit data deletion operation.

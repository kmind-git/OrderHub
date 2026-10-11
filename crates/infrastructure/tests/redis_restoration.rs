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

//! Redis adapter and persistent-cache restoration tests migrated from the retired live node.

#![cfg(feature = "redis")]

use std::sync::OnceLock;

fn redis_test_mutex() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

mod msgbus_serial_tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::mpsc,
        thread,
        time::Duration,
    };

    use nautilus_common::{
        enums::SerializationEncoding,
        msgbus::{
            self, BusPayloadType, MessageBus, MessageBusBackingFactory, MessageBusConfig,
            TypedHandler, external_io_from_backing, set_message_bus,
        },
    };
    use nautilus_core::UUID4;
    use nautilus_infrastructure::redis::{
        create_redis_connection, get_stream_key, msgbus::RedisMessageBusConfig,
    };
    use nautilus_model::{
        data::{QuoteTick, TradeTick},
        identifiers::TraderId,
    };
    use redis::aio::ConnectionManager;
    use rstest::*;

    #[fixture]
    async fn redis_connection() -> ConnectionManager {
        let config = RedisMessageBusConfig {
            connection_timeout: 1,
            number_of_retries: 0,
            ..Default::default()
        };
        create_redis_connection("msgbus-stream", &config)
            .await
            .expect("A running Redis service is required for this test")
    }

    #[ignore = "requires a dedicated Redis service; activation gate"]
    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_two_buses_publish_and_ingest_external_redis_stream(
        #[future] redis_connection: ConnectionManager,
    ) {
        let _guard = super::redis_test_mutex().lock().await;
        let _con = redis_connection.await;
        let redis_config = RedisMessageBusConfig::default();
        let trader_a = TraderId::from("NODEA-001");
        let instance_a = UUID4::new();
        let node_a_msgbus = MessageBusConfig {
            use_instance_id: true,
            stream_per_topic: false,
            ..Default::default()
        };
        let stream_key = get_stream_key(trader_a, instance_a, &node_a_msgbus);
        let node_b_msgbus = MessageBusConfig {
            external_streams: Some(vec![stream_key]),
            stream_per_topic: false,
            ..Default::default()
        };
        assert_eq!(node_a_msgbus.encoding, SerializationEncoding::Json);
        let quote = QuoteTick::default();
        let trade = TradeTick::default();
        let (ready_tx, ready_rx) = mpsc::channel::<()>();
        let (quote_tx, quote_rx) = mpsc::channel::<QuoteTick>();
        let (trade_tx, trade_rx) = mpsc::channel::<TradeTick>();

        let node_b = thread::spawn({
            let redis_config = redis_config.clone();
            move || -> anyhow::Result<()> {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()?;

                runtime.block_on(async move {
                    let bus = Rc::new(RefCell::new(MessageBus::new(
                        TraderId::from("NODEB-001"),
                        UUID4::new(),
                        None,
                        None,
                    )));
                    set_message_bus(bus.clone());
                    let backing = redis_config.create(
                        TraderId::from("NODEB-001"),
                        UUID4::new(),
                        node_b_msgbus.clone(),
                    )?;
                    let (egress, mut ingress) = external_io_from_backing(backing);
                    let mut receiver = ingress.take_receiver()?;
                    bus.borrow_mut()
                        .set_external_egress_config(egress, &node_b_msgbus)
                        .expect("valid Redis ingress configuration");
                    let received = Rc::new(Cell::new(false));
                    let quote_handler = TypedHandler::from({
                        let quote_tx = quote_tx.clone();
                        let received = received.clone();
                        move |quote: &QuoteTick| {
                            let _ = quote_tx.send(*quote);
                            received.set(true);
                        }
                    });
                    let trade_handler = TypedHandler::from(move |trade: &TradeTick| {
                        let _ = trade_tx.send(*trade);
                    });

                    msgbus::subscribe_quotes("data.quotes.*".into(), quote_handler, None);
                    msgbus::subscribe_trades("data.trades.*".into(), trade_handler, None);
                    msgbus::get_message_bus()
                        .borrow_mut()
                        .add_streaming_type(BusPayloadType::QuoteTick);
                    let result = tokio::time::timeout(Duration::from_secs(10), async {
                        ready_tx.send(()).unwrap();
                        while !received.get() {
                            let message =
                                receiver.recv().await.expect("Redis ingress remains open");
                            msgbus::republish_external_message(&message)?;
                        }
                        Ok::<(), anyhow::Error>(())
                    })
                    .await;
                    msgbus::get_message_bus().borrow_mut().dispose();

                    match result {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(e)) => Err(e),
                        Err(e) => anyhow::bail!("node B timed out: {e}"),
                    }
                })
            }
        });

        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("node B should start Redis ingress");

        let node_a = thread::spawn(move || -> anyhow::Result<()> {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?;

            runtime.block_on(async move {
                let bus = Rc::new(RefCell::new(MessageBus::new(
                    trader_a, instance_a, None, None,
                )));
                set_message_bus(bus.clone());
                let backing = redis_config.create(trader_a, instance_a, node_a_msgbus.clone())?;
                let (egress, _ingress) = external_io_from_backing(backing);
                bus.borrow_mut()
                    .set_external_egress_config(egress, &node_a_msgbus)
                    .expect("valid Redis egress configuration");

                msgbus::publish_trade("data.trades.TEST".into(), &trade);
                msgbus::publish_quote("data.quotes.TEST".into(), &quote);
                msgbus::get_message_bus().borrow_mut().dispose();

                Ok(())
            })
        });

        node_a
            .join()
            .expect("node A thread should not panic")
            .expect("node A should publish externally");
        let received_quote = quote_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("node B should republish the registered quote type");
        node_b
            .join()
            .expect("node B thread should not panic")
            .expect("node B should ingest and stop cleanly");

        assert_eq!(received_quote, quote);
        assert!(
            trade_rx.try_recv().is_err(),
            "unregistered trade type should not republish internally"
        );
    }
}

mod cache_serial_tests {
    use std::time::Duration;

    use ahash::AHashMap;
    use bytes::Bytes;
    use nautilus_common::{
        cache::{Cache, CacheConfig, database::CacheDatabaseAdapter},
        testing::wait_until_async,
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_infrastructure::redis::{
        cache::{RedisCacheConfig, RedisCacheDatabase, RedisCacheDatabaseAdapter},
        queries::DatabaseQueries,
    };
    use nautilus_model::{
        accounts::AccountAny,
        enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
        events::{
            AccountState, OrderEventAny, OrderFilled, OrderSnapshot,
            account::stubs::{cash_account_state_multi, cash_account_state_multi_changed_btc},
            position::snapshot::PositionSnapshot,
        },
        identifiers::{
            AccountId, ActorId, ClientId, ClientOrderId, PositionId, StrategyId, TradeId, TraderId,
            VenueOrderId,
        },
        instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
        orders::{Order, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        position::Position,
        types::{Money, Price, Quantity},
    };
    use redis::AsyncCommands;

    use super::redis_test_mutex;

    fn redis_cache_config() -> RedisCacheConfig {
        RedisCacheConfig {
            host: Some("localhost".to_string()),
            port: Some(6379),
            connection_timeout: 1,
            number_of_retries: 0,
            ..Default::default()
        }
    }

    async fn get_redis_cache_adapter()
    -> Result<RedisCacheDatabaseAdapter, Box<dyn std::error::Error>> {
        let mut adapter = connect_redis_cache_adapter().await?;

        // Clean the database at the start of each test
        adapter.database.flushdb().await;

        Ok(adapter)
    }

    // Connects an adapter without flushing; reconnecting under the same trader
    // key sees data written by a previous adapter (restart simulation).
    async fn connect_redis_cache_adapter()
    -> Result<RedisCacheDatabaseAdapter, Box<dyn std::error::Error>> {
        let trader_id = TraderId::from("test-trader");
        let instance_id = UUID4::new();

        let config = CacheConfig::default();
        let database =
            RedisCacheDatabase::new(trader_id, instance_id, config, redis_cache_config())
                .await
                .map_err(|e| {
                    std::io::Error::other(format!(
                        "A running Redis service is required for this test: {e}"
                    ))
                })?;

        let adapter = RedisCacheDatabaseAdapter { database };

        Ok(adapter)
    }

    #[ignore = "requires a dedicated Redis service; activation gate"]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_restart_recovery_restores_order_indexes() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let client_id = ClientId::new("BINANCE");

        let expire_time = UnixNanos::from(1_780_360_584_479_000_000_u64);
        let mut order_1 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .price(Price::from("1000.00"))
            .time_in_force(TimeInForce::Gtd)
            .expire_time(expire_time)
            .post_only(true)
            .client_order_id(ClientOrderId::new("O-19700101-000000-001-001-1"))
            .build();
        let order_2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-000000-001-001-2"))
            .build();

        let OrderEventAny::Filled(fill) = TestOrderEventStubs::filled(
            &order_1,
            &instrument,
            None,
            Some(PositionId::new("P-1")),
            None,
            None,
            None,
            None,
            None,
            None,
        ) else {
            unreachable!();
        };
        let mut position = Position::new(&instrument, fill);
        let mut account = AccountAny::from(cash_account_state_multi());

        adapter.add_instrument(&instrument).unwrap();
        adapter.add_order(&order_1, Some(client_id)).unwrap();
        adapter.add_order(&order_2, None).unwrap();
        adapter.add_position(&position).unwrap();
        adapter.add_account(&account).unwrap();
        adapter
            .index_order_position(order_1.client_order_id(), position.id)
            .unwrap();

        let submitted = TestOrderEventStubs::submitted(&order_1, AccountId::new("BINANCE-001"));
        order_1.apply(submitted.clone()).unwrap();
        adapter.update_order(&submitted).unwrap();

        let accepted = TestOrderEventStubs::accepted(
            &order_1,
            AccountId::new("BINANCE-001"),
            VenueOrderId::new("1"),
        );
        order_1.apply(accepted.clone()).unwrap();
        adapter.update_order(&accepted).unwrap();

        account
            .apply(cash_account_state_multi_changed_btc())
            .unwrap();
        adapter.update_account(&account).unwrap();

        let OrderEventAny::Filled(close_fill) = TestOrderEventStubs::filled(
            &order_2,
            &instrument,
            Some(TradeId::new("E-2")),
            Some(position.id),
            None,
            None,
            None,
            None,
            None,
            None,
        ) else {
            unreachable!();
        };
        position.apply(&close_fill);
        adapter.update_position(&position).unwrap();

        let actor_id = ActorId::new("ACTOR-001");
        assert!(adapter.load_actor(&actor_id).unwrap().is_empty());
        let mut actor_state = AHashMap::new();
        actor_state.insert("A".to_string(), Bytes::from_static(b"1"));
        adapter.update_actor(&actor_id, &actor_state).unwrap();

        let strategy_id = StrategyId::new("S-001");
        assert!(adapter.load_strategy(&strategy_id).unwrap().is_empty());
        let mut strategy_state = AHashMap::new();
        strategy_state.insert("UserState".to_string(), Bytes::from_static(b"1"));
        adapter
            .update_strategy(&strategy_id, &strategy_state)
            .unwrap();

        adapter.snapshot_order_state(&order_1).unwrap();
        adapter
            .snapshot_position_state(
                &position,
                UnixNanos::from(2_000_000_000),
                Some(Money::from("1 USD")),
            )
            .unwrap();
        adapter.heartbeat(UnixNanos::from(1_000_000_000)).unwrap();

        // Wait until the asynchronous writes land in Redis
        wait_until_async(
            || async {
                adapter.load_orders().await.unwrap().len() == 2
                    && adapter.load_positions().await.unwrap().len() == 1
                    && adapter.load_accounts().await.unwrap().len() == 1
                    && adapter
                        .load_order(&order_1.client_order_id())
                        .await
                        .unwrap()
                        .is_some_and(|loaded| loaded.status() == OrderStatus::Accepted)
                    && adapter
                        .load_position(&position.id)
                        .await
                        .unwrap()
                        .is_some_and(|loaded| loaded == position)
                    && adapter
                        .load_account(&account.id())
                        .await
                        .unwrap()
                        .is_some_and(|loaded| loaded == account)
                    && adapter.load_index_order_position().unwrap().len() == 1
                    && adapter.load_index_order_client().unwrap().len() == 1
                    && adapter.load_actor(&actor_id).unwrap() == actor_state
                    && adapter.load_strategy(&strategy_id).unwrap() == strategy_state
            },
            Duration::from_secs(5),
        )
        .await;

        let mut conn = adapter.database.con.clone();
        let encoding = adapter.database.get_encoding();
        let order_snapshot_key = format!(
            "{}:snapshots:orders:{}",
            adapter.database.trader_key,
            order_1.client_order_id()
        );
        let position_snapshot_key = format!(
            "{}:snapshots:positions:{}",
            adapter.database.trader_key, position.id
        );
        let account_key = format!("{}:accounts:{}", adapter.database.trader_key, account.id());
        let order_1_key = format!(
            "{}:orders:{}",
            adapter.database.trader_key,
            order_1.client_order_id()
        );
        let order_2_key = format!(
            "{}:orders:{}",
            adapter.database.trader_key,
            order_2.client_order_id()
        );
        let position_key = format!("{}:positions:{}", adapter.database.trader_key, position.id);
        let heartbeat_key = format!("{}:health:heartbeat", adapter.database.trader_key);

        wait_until_async(
            || {
                let mut conn = adapter.database.con.clone();
                let order_snapshot_key = order_snapshot_key.clone();
                let position_snapshot_key = position_snapshot_key.clone();
                let heartbeat_key = heartbeat_key.clone();

                async move {
                    conn.llen::<_, usize>(&order_snapshot_key)
                        .await
                        .unwrap_or(0)
                        == 1
                        && conn
                            .llen::<_, usize>(&position_snapshot_key)
                            .await
                            .unwrap_or(0)
                            == 1
                        && conn
                            .exists::<_, bool>(&heartbeat_key)
                            .await
                            .unwrap_or(false)
                }
            },
            Duration::from_secs(5),
        )
        .await;

        let account_frames: Vec<Bytes> = conn.lrange(&account_key, 0, -1).await.unwrap();
        let order_1_frames: Vec<Bytes> = conn.lrange(&order_1_key, 0, -1).await.unwrap();
        let order_2_frames: Vec<Bytes> = conn.lrange(&order_2_key, 0, -1).await.unwrap();
        let position_frames: Vec<Bytes> = conn.lrange(&position_key, 0, -1).await.unwrap();
        let order_snapshot_frames: Vec<Bytes> =
            conn.lrange(&order_snapshot_key, 0, -1).await.unwrap();
        let position_snapshot_frames: Vec<Bytes> =
            conn.lrange(&position_snapshot_key, 0, -1).await.unwrap();
        let heartbeat: String = conn.get(&heartbeat_key).await.unwrap();

        let account_events: Vec<AccountState> = account_frames
            .iter()
            .map(|frame| DatabaseQueries::deserialize_payload(encoding, frame).unwrap())
            .collect();
        let order_1_events: Vec<OrderEventAny> = order_1_frames
            .iter()
            .map(|frame| DatabaseQueries::deserialize_payload(encoding, frame).unwrap())
            .collect();
        let order_2_events: Vec<OrderEventAny> = order_2_frames
            .iter()
            .map(|frame| DatabaseQueries::deserialize_payload(encoding, frame).unwrap())
            .collect();
        let position_events: Vec<OrderFilled> = position_frames
            .iter()
            .map(|frame| DatabaseQueries::deserialize_payload(encoding, frame).unwrap())
            .collect();
        let order_snapshot: OrderSnapshot =
            DatabaseQueries::deserialize_payload(encoding, &order_snapshot_frames[0]).unwrap();
        let position_snapshot: PositionSnapshot =
            DatabaseQueries::deserialize_payload(encoding, &position_snapshot_frames[0]).unwrap();

        assert_eq!(account_events, account.events());
        assert_eq!(
            order_1_events,
            order_1.events().into_iter().cloned().collect::<Vec<_>>()
        );
        assert_eq!(
            order_2_events,
            order_2.events().into_iter().cloned().collect::<Vec<_>>()
        );
        assert_eq!(position_events, position.events.clone());
        assert_eq!(order_snapshot.client_order_id, order_1.client_order_id());
        assert_eq!(position_snapshot.position_id, position.id);
        assert_eq!(position_snapshot.ts_init, UnixNanos::from(2_000_000_000));
        assert_eq!(heartbeat, "1970-01-01T00:00:01.000000000Z");

        let order_id = order_1.client_order_id().to_string();
        assert_eq!(
            conn.sismember::<_, _, bool>(
                format!("{}:index:orders_open", adapter.database.trader_key),
                &order_id,
            )
            .await
            .unwrap(),
            order_1.is_open()
        );
        assert_eq!(
            conn.sismember::<_, _, bool>(
                format!("{}:index:orders_closed", adapter.database.trader_key),
                &order_id,
            )
            .await
            .unwrap(),
            order_1.is_closed()
        );
        assert_eq!(
            conn.sismember::<_, _, bool>(
                format!("{}:index:orders_inflight", adapter.database.trader_key),
                &order_id,
            )
            .await
            .unwrap(),
            order_1.is_inflight()
        );

        let position_id = position.id.to_string();
        assert_eq!(
            conn.sismember::<_, _, bool>(
                format!("{}:index:positions_open", adapter.database.trader_key),
                &position_id,
            )
            .await
            .unwrap(),
            position.is_open()
        );
        assert_eq!(
            conn.sismember::<_, _, bool>(
                format!("{}:index:positions_closed", adapter.database.trader_key),
                &position_id,
            )
            .await
            .unwrap(),
            position.is_closed()
        );

        // Simulate a node restart: fresh adapter over the same trader key, fresh cache
        let restarted_adapter = connect_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");
        let mut cache = Cache::new(None, Some(Box::new(restarted_adapter)));
        cache.cache_all().await.unwrap();
        cache.build_index();

        assert_eq!(
            cache.position_id(&order_1.client_order_id()),
            Some(&position.id)
        );
        assert_eq!(
            cache.client_id(&order_1.client_order_id()),
            Some(&client_id)
        );
        assert!(cache.position_id(&order_2.client_order_id()).is_none());
        assert!(cache.client_id(&order_2.client_order_id()).is_none());
        assert!(cache.order(&order_1.client_order_id()).is_some());
        assert!(cache.position(&position.id).is_some());
        let recovered_order = cache
            .order(&order_1.client_order_id())
            .expect("managed GTD order should recover after restart");
        assert_eq!(recovered_order.status(), OrderStatus::Accepted);
        assert_eq!(recovered_order.time_in_force(), TimeInForce::Gtd);
        assert_eq!(recovered_order.expire_time(), Some(expire_time));
        assert!(recovered_order.is_post_only());
        assert_eq!(
            cache.account(&account.id()).map(|loaded| loaded.cloned()),
            Some(account.clone())
        );

        let restarted_node_adapter = connect_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");
        let mut restored_cache = Cache::new(None, Some(Box::new(restarted_node_adapter)));
        restored_cache.cache_general().unwrap();
        restored_cache.cache_all().await.unwrap();
        restored_cache.build_index();
        {
            let node_cache = &restored_cache;
            assert_eq!(
                node_cache.position_id(&order_1.client_order_id()),
                Some(&position.id)
            );
            assert_eq!(
                node_cache.client_id(&order_1.client_order_id()),
                Some(&client_id)
            );
            assert!(node_cache.order(&order_1.client_order_id()).is_some());
            assert!(node_cache.position(&position.id).is_some());
            assert_eq!(
                node_cache
                    .order(&order_1.client_order_id())
                    .map(|order| order.status()),
                Some(OrderStatus::Accepted)
            );
            assert_eq!(
                node_cache
                    .account(&account.id())
                    .map(|loaded| loaded.cloned()),
                Some(account.clone())
            );
        }

        let disabled_adapter = connect_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");
        let disabled_cache = Cache::new(None, Some(Box::new(disabled_adapter)));
        assert!(disabled_cache.order(&order_1.client_order_id()).is_none());

        adapter.delete_actor(&actor_id).unwrap();
        adapter.delete_strategy(&strategy_id).unwrap();
        wait_until_async(
            || async {
                adapter.load_actor(&actor_id).unwrap().is_empty()
                    && adapter.load_strategy(&strategy_id).unwrap().is_empty()
            },
            Duration::from_secs(5),
        )
        .await;

        let flush_node_adapter = connect_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");
        let mut flushed_cache = Cache::new(
            Some(CacheConfig {
                flush_on_start: true,
                ..Default::default()
            }),
            Some(Box::new(flush_node_adapter)),
        );
        flushed_cache.flush_db();
        flushed_cache.cache_all().await.unwrap();
        flushed_cache.build_index();
        assert!(flushed_cache.order(&order_1.client_order_id()).is_none());

        let mut adapter = adapter;
        adapter.flush().unwrap();
    }
}

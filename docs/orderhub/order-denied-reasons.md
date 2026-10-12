# Order denied reasons

This reference lists the retained execution engine's `OrderDeniedCode` and `OrderDeniedReason`
values. It describes engine metadata; it does not enable additional order types or change
OrderHub's admission policy. See [admission and idempotency](admission-and-idempotency.md) and
[risk and reservations](risk-and-reservations.md) for the supported node contract.

The table is generated from `crates/model/src/events/order/denied_reason.rs`. After changing an
engine reason, regenerate it through the source test:

```bash
cargo test --locked --profile nextest -p nautilus-model regenerate_order_denied_reasons_doc -- --ignored
```

<!-- BEGIN GENERATED: order-denied-reasons -->

| Code                                             | Description                                                                           |
| ------------------------------------------------ | ------------------------------------------------------------------------------------- |
| `PRICE_PRECISION_EXCEEDS_MAXIMUM`                | The price precision exceeds the instrument maximum.                                   |
| `PRICE_NOT_POSITIVE`                             | The price is not positive.                                                            |
| `QUANTITY_PRECISION_EXCEEDS_MAXIMUM`             | The quantity precision exceeds the instrument maximum.                                |
| `QUANTITY_CONVERSION_FAILED`                     | The order quantity could not be converted for risk checks.                            |
| `QUANTITY_EXCEEDS_MAXIMUM`                       | The effective order quantity exceeds the instrument maximum.                          |
| `QUANTITY_BELOW_MINIMUM`                         | The effective order quantity is below the instrument minimum.                         |
| `INVALID_MAX_NOTIONAL_PER_ORDER`                 | The configured maximum notional per order is invalid.                                 |
| `MISSING_EXPIRE_TIME`                            | A GTD order is missing its expire time.                                               |
| `EXPIRE_TIME_IN_PAST`                            | The order's expire time is in the past.                                               |
| `MISSING_TRAILING_OFFSET_TYPE`                   | The order is missing a required trailing offset type.                                 |
| `UNSUPPORTED_TRAILING_OFFSET_TYPE`               | The order's trailing offset type is not supported.                                    |
| `MISSING_TRIGGER_TYPE`                           | The order is missing a required trigger type.                                         |
| `MISSING_TRAILING_OFFSET`                        | The order is missing a required trailing offset.                                      |
| `INSTRUMENT_NOT_FOUND`                           | The instrument was not found in the cache.                                            |
| `POSITION_NOT_FOUND`                             | The position for a reduce-only order was not found.                                   |
| `MARKET_PRICE_UNAVAILABLE`                       | No market price is available for the order risk check.                                |
| `TRAILING_STOP_CALCULATION_FAILED`               | The trailing stop trigger price could not be calculated.                              |
| `NOTIONAL_CALCULATION_FAILED`                    | The order notional value could not be calculated.                                     |
| `NOTIONAL_BELOW_MINIMUM`                         | The order notional is below the instrument minimum.                                   |
| `NOTIONAL_EXCEEDS_MAXIMUM`                       | The order notional exceeds the instrument maximum.                                    |
| `NOTIONAL_EXCEEDS_MAX_PER_ORDER`                 | The order notional exceeds the configured maximum per order.                          |
| `NOTIONAL_EXCEEDS_FREE_BALANCE`                  | The order notional exceeds the account free balance.                                  |
| `INITIAL_MARGIN_CALCULATION_FAILED`              | The order initial margin could not be calculated.                                     |
| `INITIAL_MARGIN_EXCEEDS_FREE_BALANCE`            | The order initial margin exceeds the account free balance.                            |
| `BETTING_BALANCE_LOCKED_CALCULATION_FAILED`      | The balance to lock for the betting order could not be calculated.                    |
| `CUMULATIVE_NOTIONAL_EXCEEDS_FREE_BALANCE`       | The cumulative order notional exceeds the account free balance.                       |
| `CUMULATIVE_INITIAL_MARGIN_CALCULATION_FAILED`   | The cumulative initial margin could not be calculated.                                |
| `CUMULATIVE_INITIAL_MARGIN_EXCEEDS_FREE_BALANCE` | The cumulative initial margin exceeds the account free balance.                       |
| `REDUCE_ONLY_WOULD_INCREASE_POSITION`            | A reduce-only order would increase the position.                                      |
| `ORDER_LIST_INCOMPLETE`                          | The order list is missing orders in the cache.                                        |
| `ORDER_LIST_DENIED`                              | The order was denied because its order list failed risk checks.                       |
| `TRADING_HALTED`                                 | Trading is halted; new submissions and modifications are denied.                      |
| `TRADING_STATE_REDUCING`                         | Trading is reducing; only eligible reduce-only submissions are permitted.             |
| `RATE_LIMIT_EXCEEDED`                            | The order submission rate limit was exceeded.                                         |
| `STREAM_RECONCILING`                             | The execution stream is unavailable or recovering; retry after recovery.              |
| `NO_EXECUTION_CLIENT`                            | No execution client was found for the routed command.                                 |
| `CLIENT_VENUE_MISMATCH`                          | The execution client does not handle the order venue.                                 |
| `SUBMIT_FAILED`                                  | Submitting the order to the execution client failed.                                  |
| `INVALID_CLIENT_ORDER_ID`                        | The client order ID is invalid for the venue.                                         |
| `INVALID_POSITION_ID`                            | The supplied position ID is invalid for the order submission.                         |
| `UNSUPPORTED_ORDER_LIST`                         | The venue does not support the requested order list.                                  |
| `UNSUPPORTED_ORDER_TYPE`                         | The order type is not supported.                                                      |
| `UNSUPPORTED_REDUCE_ONLY`                        | The execution client or venue does not support the requested reduce-only instruction. |
| `UNSUPPORTED_TIME_IN_FORCE`                      | The order's time in force is not supported.                                           |
| `UNSUPPORTED_TP_SL`                              | The venue does not support the requested take-profit/stop-loss parameters.            |
| `VALIDATION_FAILED`                              | The order failed validation before submission.                                        |

<!-- END GENERATED: order-denied-reasons -->

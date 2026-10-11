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

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_common::msgbus::matching::{is_matching, is_matching_backtracking};
use std::hint::black_box;

fn bench_matching_exact() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.quotes.BINANCE.ETHUSDT";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_star_end() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.quotes.BINANCE.*";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_star_middle() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.*.BINANCE.ETHUSDT";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_multi_star() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.*.BINANCE.*";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_question() {
    let topic = b"data.quotes.BINANCE.ETHUSD";
    let pattern = b"data.quotes.BINANCE.ETHUS?";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_multi_question() {
    let topic = b"data.quotes.BINANCE.ETHUSD";
    let pattern = b"data.quotes.BINANCE.ETH???";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_mixed() {
    let topic = b"data.quotes.BINANCE.ETHUSD";
    let pattern = b"data.*.BINANCE.ETH???";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_realistic() {
    let topic = b"data.trades.BINANCE.ETHUSDT";
    let pattern = b"data.*.BINANCE.ETH*";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_no_match() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.trades.BYBIT.*";
    black_box(is_matching(topic, pattern));
}

fn bench_mstr_matching_exact() {
    black_box(is_matching_backtracking(
        "data.quotes.BINANCE.ETHUSDT".into(),
        "data.quotes.BINANCE.ETHUSDT".into(),
    ));
}

fn bench_mstr_matching_star_middle() {
    black_box(is_matching_backtracking(
        "data.quotes.BINANCE.ETHUSDT".into(),
        "data.*.BINANCE.ETHUSDT".into(),
    ));
}

fn bench_mstr_matching_mixed() {
    black_box(is_matching_backtracking(
        "data.quotes.BINANCE.ETHUSD".into(),
        "data.*.BINANCE.ETH???".into(),
    ));
}

fn bench_mstr_matching_realistic() {
    black_box(is_matching_backtracking(
        "data.trades.BINANCE.ETHUSDT".into(),
        "data.*.BINANCE.ETH*".into(),
    ));
}

// These historical filenames now measure elapsed time, not Valgrind instructions.
fn benchmarks(c: &mut Criterion) {
    c.bench_function("bench_matching_exact", |b| {
        b.iter(bench_matching_exact);
    });
    c.bench_function("bench_matching_star_end", |b| {
        b.iter(bench_matching_star_end);
    });
    c.bench_function("bench_matching_star_middle", |b| {
        b.iter(bench_matching_star_middle);
    });
    c.bench_function("bench_matching_multi_star", |b| {
        b.iter(bench_matching_multi_star);
    });
    c.bench_function("bench_matching_question", |b| {
        b.iter(bench_matching_question);
    });
    c.bench_function("bench_matching_multi_question", |b| {
        b.iter(bench_matching_multi_question);
    });
    c.bench_function("bench_matching_mixed", |b| {
        b.iter(bench_matching_mixed);
    });
    c.bench_function("bench_matching_realistic", |b| {
        b.iter(bench_matching_realistic);
    });
    c.bench_function("bench_matching_no_match", |b| {
        b.iter(bench_matching_no_match);
    });
    c.bench_function("bench_mstr_matching_exact", |b| {
        b.iter(bench_mstr_matching_exact);
    });
    c.bench_function("bench_mstr_matching_star_middle", |b| {
        b.iter(bench_mstr_matching_star_middle);
    });
    c.bench_function("bench_mstr_matching_mixed", |b| {
        b.iter(bench_mstr_matching_mixed);
    });
    c.bench_function("bench_mstr_matching_realistic", |b| {
        b.iter(bench_mstr_matching_realistic);
    });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
